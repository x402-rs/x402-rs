use axum::body::Body;
use axum::extract::Request;
use axum::response::{IntoResponse, Response};
use http::{HeaderMap, HeaderValue, StatusCode, Uri};
use serde_json::json;
use std::convert::Infallible;
use std::sync::Arc;
use tower::Service;
use url::Url;
use x402_rs::facilitator::Facilitator;
use x402_rs::proto;
use x402_rs::proto::v1::V1PriceTag;
use x402_rs::proto::v2::ResourceInfo;
use x402_rs::proto::{v1, v2};
use x402_rs::util::Base64Bytes;

#[derive(Debug, Clone)]
pub struct ResourceInfoBuilder {
    pub description: String,
    pub mime_type: String,
    pub url: Option<String>,
}

impl Default for ResourceInfoBuilder {
    fn default() -> Self {
        Self {
            description: "".to_string(),
            mime_type: "application/json".to_string(),
            url: None,
        }
    }
}

impl ResourceInfoBuilder {
    // Determine the resource URL (static or dynamic)
    pub fn as_resource_info(&self, base_url: &Url, request_uri: &Uri) -> v2::ResourceInfo {
        v2::ResourceInfo {
            description: self.description.clone(),
            mime_type: self.mime_type.clone(),
            url: self.url.clone().unwrap_or_else(|| {
                let mut url = base_url.clone();
                url.set_path(request_uri.path());
                url.set_query(request_uri.query());
                url.to_string()
            }),
        }
    }
}

pub struct V1Paygate<TFacilitator> {
    pub facilitator: TFacilitator,
    pub settle_before_execution: bool,
    pub accepts: Arc<Vec<V1PriceTag>>,
    pub resource: ResourceInfo,
}

impl<TFacilitator> V1Paygate<TFacilitator> {
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

impl<TFacilitator> V1Paygate<TFacilitator>
where
    TFacilitator: Facilitator,
{
    const PAYMENT_HEADER_NAME: &'static str = "X-PAYMENT";

    #[cfg_attr(
        feature = "telemetry",
        instrument(name = "x402.handle_request", skip_all)
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
    ) -> Result<Response, PaygateError>
    where
        S::Response: IntoResponse,
        S::Error: IntoResponse,
        S::Future: Send,
    {
        // Extract payment payload from headers
        let header = extract_payment_header(req.headers(), Self::PAYMENT_HEADER_NAME).ok_or(
            VerificationError::PaymentHeaderRequired(Self::PAYMENT_HEADER_NAME),
        )?;
        let payment_payload = extract_payment_payload::<v1::PaymentPayload>(header)
            .ok_or(VerificationError::InvalidPaymentHeader)?;

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

            let verify_response_v1: v1::VerifyResponse = verify_response
                .try_into()
                .map_err(|e| VerificationError::VerificationFailed(format!("{e}")))?;

            let verify_request = match verify_response_v1 {
                v1::VerifyResponse::Valid { .. } => Ok(verify_request),
                v1::VerifyResponse::Invalid { reason, .. } => {
                    Err(VerificationError::VerificationFailed(reason))
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
        payment_payload: v1::PaymentPayload,
    ) -> Result<proto::VerifyRequest, VerificationError> {
        let selected = self
            .payment_requirements()
            .into_iter()
            .find(|requirement| {
                requirement.scheme == payment_payload.scheme
                    && requirement.network == payment_payload.network
            })
            .ok_or(VerificationError::NoPaymentMatching)?;
        let verify_request = v1::VerifyRequest {
            x402_version: v1::X402Version1,
            payment_payload,
            payment_requirements: selected,
        };
        let verify_request = verify_request
            .try_into()
            .map_err(|e| VerificationError::VerificationFailed(format!("{e}")))?;
        Ok(verify_request)
    }

    pub async fn verify_payment(
        &self,
        verify_request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, VerificationError> {
        let verify_response = self
            .facilitator
            .verify(verify_request)
            .await
            .map_err(|e| VerificationError::VerificationFailed(format!("{e}")))?;
        Ok(verify_response)
    }

    pub async fn settle_payment(
        &self,
        settle_request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, PaygateError> {
        let settle_response = self
            .facilitator
            .settle(settle_request)
            .await
            .map_err(|e| PaygateError::Settlement(format!("{e}")))?;
        Ok(settle_response)
    }

    pub fn error_into_response(&self, err: PaygateError) -> Response {
        match err {
            PaygateError::Verification(err) => {
                let payment_required_response = v1::PaymentRequired {
                    error: Some(err.to_string()),
                    accepts: self.payment_requirements(),
                    x402_version: v1::X402Version1,
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
            PaygateError::Settlement(err) => {
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

    pub fn payment_requirements(&self) -> Vec<v1::PaymentRequirements> {
        self.accepts
            .iter()
            .map(|price_tag| v1::PaymentRequirements {
                scheme: price_tag.scheme.clone(),
                network: price_tag.network.clone(),
                max_amount_required: price_tag.amount.clone(),
                resource: self.resource.url.clone(),
                description: self.resource.description.clone(),
                mime_type: self.resource.mime_type.clone(),
                output_schema: None,
                pay_to: price_tag.pay_to.clone(),
                max_timeout_seconds: price_tag.max_timeout_seconds,
                asset: price_tag.asset.clone(),
                extra: price_tag.extra.clone(),
            })
            .collect::<Vec<_>>()
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
enum PaygateError {
    #[error(transparent)]
    Verification(#[from] VerificationError),
    #[error("Settlement failed: {0}")]
    Settlement(String),
}

#[derive(Debug, thiserror::Error)]
enum VerificationError {
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
fn settlement_to_header(settlement: proto::SettleResponse) -> Result<HeaderValue, PaygateError> {
    let json =
        serde_json::to_vec(&settlement).map_err(|err| PaygateError::Settlement(err.to_string()))?;
    let payment_header = Base64Bytes::encode(json);
    HeaderValue::from_bytes(payment_header.as_ref())
        .map_err(|err| PaygateError::Settlement(err.to_string()))
}
