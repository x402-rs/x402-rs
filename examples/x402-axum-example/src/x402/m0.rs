use axum::response::{IntoResponse, Response};
use http::{HeaderMap, HeaderValue};
use tower::Service;
use url::Url;
use x402_rs::facilitator::Facilitator;
use x402_rs::proto;
use x402_rs::proto::v1;
use x402_rs::util::Base64Bytes;

use crate::x402::paygate_error::PaygateError;

/// A service-level helper struct responsible for verifying and settling
/// x402 payments based on request headers and known payment requirements.
pub struct X402Paygate<TPaymentRequirements, TFacilitator> {
    pub facilitator: TFacilitator,
    /// Whether to settle payment before executing the request (true) or after (false)
    pub payment_requirements: Vec<TPaymentRequirements>,

    pub settle_before_execution: bool,
    pub description: Option<String>,
    pub mime_type: Option<String>, // TODO ARC!!
    /// Optional resource URL. If not set, it will be derived from a request URI.
    pub resource: Option<Url>,
}

impl<TPaymentRequirements, TFacilitator> X402Paygate<TPaymentRequirements, TFacilitator> {
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

impl<TFacilitator> X402Paygate<v1::PaymentRequirements, TFacilitator>
where
    TFacilitator: Facilitator,
{
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
    ) -> Result<Response, PaygateError<v1::PaymentRequirements>>
    where
        S::Response: IntoResponse,
        S::Error: IntoResponse,
        S::Future: Send,
    {
        // Extract payment payload from headers
        let header = extract_payment_header(req.headers(), "X-Payment").ok_or(
            PaygateError::payment_header_required(self.payment_requirements.clone()),
        )?;
        let payment_payload = extract_payment_payload::<v1::PaymentPayload>(header).ok_or(
            PaygateError::invalid_payment_header(self.payment_requirements.clone()),
        )?;
        // Verify the payment meets requirements
        let verify_request = self.verify_payment(payment_payload).await?;

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

    /// Finds the payment requirement entry matching the given payload's scheme and network.
    fn find_matching_payment_requirements(
        &self,
        payment_payload: &v1::PaymentPayload,
    ) -> Option<&v1::PaymentRequirements> {
        self.payment_requirements.iter().find(|requirement| {
            requirement.scheme == payment_payload.scheme
                && requirement.network == payment_payload.network
        })
    }

    /// Verifies the provided payment using the facilitator and known requirements. Returns a [`VerifyRequest`] if the payment is valid.
    #[cfg_attr(
        feature = "telemetry",
        instrument(name = "x402.verify_payment", skip_all, err)
    )]
    pub async fn verify_payment(
        &self,
        payment_payload: v1::PaymentPayload,
    ) -> Result<proto::VerifyRequest, PaygateError<v1::PaymentRequirements>> {
        let selected = self
            .find_matching_payment_requirements(&payment_payload)
            .ok_or(PaygateError::no_payment_matching(
                self.payment_requirements.clone(),
            ))?;

        let verify_request = v1::VerifyRequest {
            x402_version: v1::X402Version1,
            payment_payload: payment_payload,
            payment_requirements: selected,
        };

        let verify_request = verify_request
            .try_into()
            .map_err(|e| PaygateError::verification_failed(e, self.payment_requirements.clone()))?;

        let verify_response = self
            .facilitator
            .verify(&verify_request)
            .await
            .map_err(|e| PaygateError::verification_failed(e, self.payment_requirements.clone()))?;

        let verify_response_v1: v1::VerifyResponse = verify_response
            .try_into()
            .map_err(|e| PaygateError::verification_failed(e, self.payment_requirements.clone()))?;

        match verify_response_v1 {
            v1::VerifyResponse::Valid { .. } => Ok(verify_request),
            v1::VerifyResponse::Invalid { reason, .. } => Err(PaygateError::verification_failed(
                reason,
                self.payment_requirements.clone(),
            )),
        }
    }

    /// Attempts to settle a verified payment on-chain. Returns [`SettleResponse`] on success or emits a 402 error.
    #[cfg_attr(
        feature = "telemetry",
        instrument(name = "x402.settle_payment", skip_all, err)
    )]
    pub async fn settle_payment(
        &self,
        settle_request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, PaygateError<v1::PaymentRequirements>> {
        let settle_response: proto::SettleResponse = self
            .facilitator
            .settle(settle_request)
            .await
            .map_err(|e| PaygateError::settlement_failed(e))?;
        let settle_response_v1: v1::SettleResponse =
            serde_json::from_value(settle_response.0.clone()).unwrap();

        match settle_response_v1 {
            v1::SettleResponse::Success { .. } => Ok(settle_response),
            v1::SettleResponse::Error { reason, .. } => Err(PaygateError::settlement_failed(reason)),
        }
    }
}

/// Converts a [`proto::SettleResponse`] into an HTTP header value.
///
/// Returns an error response if conversion fails.
fn settlement_to_header(
    settlement: proto::SettleResponse,
) -> Result<HeaderValue, PaygateError<v1::PaymentRequirements>> {
    let json = serde_json::to_vec(&settlement).map_err(|err| PaygateError::settlement_failed(err))?;
    let payment_header = Base64Bytes::encode(json);

    HeaderValue::from_bytes(payment_header.as_ref())
        .map_err(|err| PaygateError::settlement_failed(err))
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
