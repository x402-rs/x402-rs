use axum::body::Body;
use axum::response::{IntoResponse, Response};
use http::{HeaderMap, HeaderValue, StatusCode};
use serde_json::json;
use std::convert::Infallible;
use std::sync::Arc;
use tower::Service;
use x402_rs::facilitator::Facilitator;
use x402_rs::proto;
use x402_rs::proto::v2;
use x402_rs::util::Base64Bytes;

use crate::x402::v2_eip155_exact::V2PriceTag;

pub struct V2Paygate<TFacilitator> {
    pub facilitator: TFacilitator,
    pub settle_before_execution: bool,
    pub accepts: Arc<Vec<V2PriceTag>>,
    pub resource: v2::ResourceInfo,
}

impl<TFacilitator> V2Paygate<TFacilitator> {
    /// Calls the inner service with proper telemetry instrumentation.
    async fn call_inner<
        ReqBody,
        ResBody,
        S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>>,
    >(
        mut inner: S,
        req: http::Request<ReqBody>,
    ) -> Result<http::Response<ResBody>, S::Error>
    where
        S::Future: Send,
    {
        #[cfg(feature = "telemetry")]
        {
            inner
                .call(req)
                .instrument(tracing::info_span!("inner"))
                .await
        }
        #[cfg(not(feature = "telemetry"))]
        {
            inner.call(req).await
        }
    }
}

impl<TFacilitator> V2Paygate<TFacilitator>
where
    TFacilitator: Facilitator,
{
    const PAYMENT_HEADER_NAME: &'static str = "X-PAYMENT";

    #[cfg_attr(
        feature = "telemetry",
        instrument(name = "x402.v2.handle_request", skip_all)
    )]
    pub async fn handle_request<
        ReqBody,
        ResBody,
        S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>>,
    >(
        self,
        inner: S,
        req: http::Request<ReqBody>,
    ) -> Result<Response, Infallible>
    where
        S::Response: IntoResponse,
        S::Error: IntoResponse,
        S::Future: Send,
    {
        match self.handle_request_fallible(inner, req).await {
            Ok(response) => Ok(response),
            Err(err) => Ok(self.error_into_response(err)),
        }
    }

    pub async fn handle_request_fallible<
        ReqBody,
        ResBody,
        S: Service<http::Request<ReqBody>, Response = http::Response<ResBody>>,
    >(
        &self,
        inner: S,
        req: http::Request<ReqBody>,
    ) -> Result<Response, PaygateV2Error>
    where
        S::Response: IntoResponse,
        S::Error: IntoResponse,
        S::Future: Send,
    {
        // Extract payment payload from headers
        let header = extract_payment_header(req.headers(), Self::PAYMENT_HEADER_NAME).ok_or(
            VerificationV2Error::PaymentHeaderRequired(Self::PAYMENT_HEADER_NAME),
        )?;
        let payment_payload = extract_payment_payload::<v2::PaymentPayload<v2::PaymentRequirements, serde_json::Value>>(header)
            .ok_or(VerificationV2Error::InvalidPaymentHeader)?;

        let verify_request = self.make_verify_request(payment_payload)?;

        if self.settle_before_execution {
            // Settlement before execution: settle payment first, then call inner handler
            #[cfg(feature = "telemetry")]
            tracing::debug!("Settling payment before request execution");

            let settlement = self.settle_payment(&verify_request).await?;

            let header_value = settlement_to_header(settlement)?;

            // Settlement succeeded, now execute the request
            let response = match Self::call_inner(inner, req).await {
                Ok(response) => response,
                Err(err) => return Ok(err.into_response()),
            };

            // Add payment response header
            let mut res = response;
            res.headers_mut().insert("X-Payment-Response", header_value);
            Ok(res.into_response())
        } else {
            // Settlement after execution (default): call inner handler first, then settle
            #[cfg(feature = "telemetry")]
            tracing::debug!("Settling payment after request execution");

            let verify_response = self.verify_payment(&verify_request).await?;

            let verify_response_v2: v2::VerifyResponse = verify_response
                .try_into()
                .map_err(|e| VerificationV2Error::VerificationFailed(format!("{e}")))?;

            let verify_request = match verify_response_v2 {
                v2::VerifyResponse::Valid { .. } => Ok(verify_request),
                v2::VerifyResponse::Invalid { reason, .. } => {
                    Err(VerificationV2Error::VerificationFailed(reason))
                }
            }?;

            let response = match Self::call_inner(inner, req).await {
                Ok(response) => response,
                Err(err) => return Ok(err.into_response()),
            };

            if response.status().is_client_error() || response.status().is_server_error() {
                return Ok(response.into_response());
            }

            let settlement = self.settle_payment(&verify_request).await?;

            let header_value = settlement_to_header(settlement)?;

            let mut res = response;
            res.headers_mut().insert("X-Payment-Response", header_value);
            Ok(res.into_response())
        }
    }

    fn make_verify_request(
        &self,
        payment_payload: v2::PaymentPayload<v2::PaymentRequirements, serde_json::Value>,
    ) -> Result<proto::VerifyRequest, VerificationV2Error> {
        // In V2, the accepted requirements are embedded in the payload
        let accepted = &payment_payload.accepted;

        // Find matching requirements from our accepts list
        let selected = self
            .accepts
            .iter()
            .find(|requirement| {
                requirement.scheme == accepted.scheme
                    && requirement.network == accepted.network
            })
            .ok_or(VerificationV2Error::NoPaymentMatching)?;

        // Build the V2 verify request
        let verify_request = v2::VerifyRequest {
            x402_version: v2::X402Version2,
            payment_payload,
            payment_requirements: selected.clone(),
        };

        let json = serde_json::to_value(&verify_request)
            .map_err(|e| VerificationV2Error::VerificationFailed(format!("{e}")))?;

        Ok(proto::VerifyRequest::from(json))
    }

    pub async fn verify_payment(
        &self,
        verify_request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, VerificationV2Error> {
        let verify_response = self
            .facilitator
            .verify(verify_request)
            .await
            .map_err(|e| VerificationV2Error::VerificationFailed(format!("{e}")))?;
        Ok(verify_response)
    }

    pub async fn settle_payment(
        &self,
        settle_request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, PaygateV2Error> {
        let settle_response = self
            .facilitator
            .settle(settle_request)
            .await
            .map_err(|e| PaygateV2Error::Settlement(format!("{e}")))?;
        Ok(settle_response)
    }

    pub fn error_into_response(&self, err: PaygateV2Error) -> Response {
        match err {
            PaygateV2Error::Verification(err) => {
                let payment_required_response = v2::PaymentRequired {
                    error: Some(err.to_string()),
                    accepts: self.accepts.iter().cloned().collect(),
                    x402_version: v2::X402Version2,
                    resource: self.resource.clone(),
                };
                let payment_required_response_bytes =
                    serde_json::to_vec(&payment_required_response).expect("serialization failed");
                let body = Body::from(payment_required_response_bytes);
                Response::builder()
                    .status(StatusCode::PAYMENT_REQUIRED)
                    .header("Content-Type", "application/json")
                    .body(body)
                    .expect("Fail to construct response")
            }
            PaygateV2Error::Settlement(err) => {
                let body = Body::from(
                    json!({
                        "error": "Settlement failed",
                        "details": err.to_string()
                    })
                    .to_string(),
                );
                Response::builder()
                    .status(StatusCode::PAYMENT_REQUIRED)
                    .header("Content-Type", "application/json")
                    .body(body)
                    .expect("Fail to construct response")
            }
        }
    }
}

fn extract_payment_header<'a>(header_map: &'a HeaderMap, header_name: &'a str) -> Option<&'a [u8]> {
    header_map.get(header_name).map(|h| h.as_bytes())
}

fn extract_payment_payload<T>(header_bytes: &[u8]) -> Option<T>
where
    T: serde::de::DeserializeOwned,
{
    let base64 = Base64Bytes::from(header_bytes).decode().ok()?;
    let value = serde_json::from_slice(base64.as_ref()).ok()?;
    Some(value)
}

#[derive(Debug, thiserror::Error)]
pub enum PaygateV2Error {
    #[error(transparent)]
    Verification(#[from] VerificationV2Error),
    #[error("Settlement failed: {0}")]
    Settlement(String),
}

#[derive(Debug, thiserror::Error)]
pub enum VerificationV2Error {
    #[error("{0} header is required")]
    PaymentHeaderRequired(&'static str),
    #[error("Invalid or malformed payment header")]
    InvalidPaymentHeader,
    #[error("Unable to find matching payment requirements")]
    NoPaymentMatching,
    #[error("Verification failed: {0}")]
    VerificationFailed(String),
}

/// Converts a [`proto::SettleResponse`] into an HTTP header value.
///
/// Returns an error response if conversion fails.
fn settlement_to_header(settlement: proto::SettleResponse) -> Result<HeaderValue, PaygateV2Error> {
    let json =
        serde_json::to_vec(&settlement).map_err(|err| PaygateV2Error::Settlement(err.to_string()))?;
    let payment_header = Base64Bytes::encode(json);
    HeaderValue::from_bytes(payment_header.as_ref())
        .map_err(|err| PaygateV2Error::Settlement(err.to_string()))
}
