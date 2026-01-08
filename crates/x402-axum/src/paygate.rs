//! Unified Paygate implementation supporting both V1 and V2 x402 protocols.
//!
//! This module provides a trait-based abstraction that allows sharing the core
//! payment gate logic between protocol versions while allowing version-specific
//! behavior through the [`PaygateProtocol`] trait.
//!
//! ## Overview
//!
//! The paygate handles:
//! - Extracting payment headers from requests
//! - Verifying payments with the facilitator
//! - Settling payments on-chain
//! - Returning appropriate 402 responses when payment is required
//!
//! ## Example
//!
//! ```ignore
//! use x402_axum::paygate::{Paygate, PaygateProtocol};
//!
//! // Create a paygate for V1 or V2 protocol
//! let paygate = Paygate {
//!     facilitator,
//!     settle_before_execution: false,
//!     accepts: Arc::new(price_tags),
//!     resource: ResourceInfoBuilder::default().as_resource_info(&base_url, &uri),
//! };
//!
//! // Handle a request
//! let response = paygate.handle_request(inner, request).await;
//! ```

use axum_core::body::Body;
use axum_core::response::{IntoResponse, Response};
use http::{HeaderMap, HeaderValue, StatusCode, Uri};
use serde_json::json;
use std::convert::Infallible;
use std::sync::Arc;
use tower::Service;
use url::Url;
use x402_rs::chain::ChainId;
use x402_rs::facilitator::Facilitator;
use x402_rs::proto;
use x402_rs::proto::{SupportedResponse, v1, v2};
use x402_rs::util::Base64Bytes;

#[cfg(feature = "telemetry")]
use tracing::Instrument;
#[cfg(feature = "telemetry")]
use tracing::instrument;

// ============================================================================
// Common Types
// ============================================================================

/// Builder for resource information that can be used with both V1 and V2 protocols.
#[derive(Debug, Clone)]
pub struct ResourceInfoBuilder {
    /// Description of the protected resource
    pub description: String,
    /// MIME type of the protected resource
    pub mime_type: String,
    /// Optional explicit URL of the protected resource
    pub url: Option<String>,
}

// FIXME Matching for v2??
// FIXME Partial, FUll, dynamic price tag offers

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
    /// Determines the resource URL (static or dynamic).
    ///
    /// If `url` is set, returns it directly. Otherwise, constructs a URL by combining
    /// the base URL with the request URI's path and query.
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

// ============================================================================
// Error Types
// ============================================================================

/// Common verification errors shared between protocol versions.
#[derive(Debug, thiserror::Error)]
pub enum VerificationError {
    #[error("{0} header is required")]
    PaymentHeaderRequired(&'static str),
    #[error("Invalid or malformed payment header")]
    InvalidPaymentHeader,
    #[error("Unable to find matching payment requirements")]
    NoPaymentMatching,
    #[error("Verification failed: {0}")]
    VerificationFailed(String),
}

/// Paygate error type that wraps verification and settlement errors.
#[derive(Debug, thiserror::Error)]
pub enum PaygateError {
    #[error(transparent)]
    Verification(#[from] VerificationError),
    #[error("Settlement failed: {0}")]
    Settlement(String),
}

// ============================================================================
// PaygateProtocol Trait
// ============================================================================

/// Trait defining version-specific behavior for the x402 payment gate.
///
/// This trait is implemented directly on the price tag types (`V1PriceTag` and
/// `V2PriceTag`/`v2::PaymentRequirements`), allowing the core payment gate logic
/// to be shared while version-specific behavior is implemented separately.
pub trait PaygateProtocol: Clone + Send + Sync + 'static {
    /// The payment payload type extracted from the request header.
    type PaymentPayload: serde::de::DeserializeOwned + Send;

    /// The HTTP header name for the payment payload.
    const PAYMENT_HEADER_NAME: &'static str;

    /// Constructs a verify request from the payment payload and accepted requirements.
    ///
    /// The `resource` parameter provides resource information that may be needed
    /// for protocol-specific requirements (e.g., V1 includes resource info in PaymentRequirements).
    fn make_verify_request(
        payload: Self::PaymentPayload,
        accepts: &[Self],
        resource: &v2::ResourceInfo,
    ) -> Result<proto::VerifyRequest, VerificationError>;

    /// Converts an error into an HTTP response with appropriate format.
    fn error_into_response(
        err: PaygateError,
        accepts: &[Self],
        resource: &v2::ResourceInfo,
    ) -> Response;

    /// Converts the verify response to the protocol-specific format and validates it.
    fn validate_verify_response(
        verify_response: proto::VerifyResponse,
    ) -> Result<(), VerificationError>;

    /// Enriches a price tag with facilitator capabilities.
    ///
    /// Called by middleware when building 402 response to add extra information like fee payer
    /// from the facilitator's supported endpoints.
    fn enrich_with_capabilities(price_tag: &Self, capabilities: &SupportedResponse) -> Self;
}

// ============================================================================
// V1 Protocol Implementation (on v1::PriceTag)
// ============================================================================

impl PaygateProtocol for v1::PriceTag {
    type PaymentPayload = v1::PaymentPayload;

    const PAYMENT_HEADER_NAME: &'static str = "X-PAYMENT";

    fn make_verify_request(
        payment_payload: Self::PaymentPayload,
        accepts: &[Self],
        resource: &v2::ResourceInfo,
    ) -> Result<proto::VerifyRequest, VerificationError> {
        let selected = accepts
            .iter()
            .find(|requirement| {
                requirement.scheme == payment_payload.scheme
                    && requirement.network == payment_payload.network
            })
            .ok_or(VerificationError::NoPaymentMatching)?;

        let verify_request = v1::VerifyRequest {
            x402_version: v1::X402Version1,
            payment_payload,
            payment_requirements: price_tag_to_v1_requirements_with_resource(selected, resource),
        };

        verify_request
            .try_into()
            .map_err(|e| VerificationError::VerificationFailed(format!("{e}")))
    }

    fn error_into_response(
        err: PaygateError,
        accepts: &[Self],
        resource: &v2::ResourceInfo,
    ) -> Response {
        match err {
            PaygateError::Verification(err) => {
                let payment_required_response = v1::PaymentRequired {
                    error: Some(err.to_string()),
                    accepts: accepts
                        .iter()
                        .map(|pt| price_tag_to_v1_requirements_with_resource(pt, resource))
                        .collect(),
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

    fn validate_verify_response(
        verify_response: proto::VerifyResponse,
    ) -> Result<(), VerificationError> {
        let verify_response_v1: v1::VerifyResponse = verify_response
            .try_into()
            .map_err(|e| VerificationError::VerificationFailed(format!("{e}")))?;

        match verify_response_v1 {
            v1::VerifyResponse::Valid { .. } => Ok(()),
            v1::VerifyResponse::Invalid { reason, .. } => {
                Err(VerificationError::VerificationFailed(reason))
            }
        }
    }

    fn enrich_with_capabilities(price_tag: &Self, capabilities: &SupportedResponse) -> Self {
        let mut enriched = price_tag.clone();

        // Only enrich if extra is None (not already set)
        if enriched.extra.is_some() {
            return enriched;
        }

        // Find fee_payer for this network from capabilities.signers
        let chain_id = ChainId::from_network_name(&price_tag.network);
        if let Some(chain_id) = chain_id
            && let Some(signers) = capabilities.signers.get(&chain_id)
            && let Some(fee_payer) = signers.first()
        {
            let extra = serde_json::json!({ "feePayer": fee_payer });
            enriched.extra = serde_json::to_string(&extra)
                .ok()
                .and_then(|s| serde_json::value::RawValue::from_string(s).ok());
        }

        enriched
    }
}

/// Helper function to convert V1PriceTag to v1::PaymentRequirements with resource info.
fn price_tag_to_v1_requirements_with_resource(
    price_tag: &v1::PriceTag,
    resource: &v2::ResourceInfo,
) -> v1::PaymentRequirements {
    v1::PaymentRequirements {
        scheme: price_tag.scheme.clone(),
        network: price_tag.network.clone(),
        max_amount_required: price_tag.amount.clone(),
        resource: resource.url.clone(),
        description: resource.description.clone(),
        mime_type: resource.mime_type.clone(),
        output_schema: None,
        pay_to: price_tag.pay_to.clone(),
        max_timeout_seconds: price_tag.max_timeout_seconds,
        asset: price_tag.asset.clone(),
        extra: price_tag.extra.clone(),
    }
}

// ============================================================================
// V2 Protocol Implementation (on v2::PaymentRequirements / V2PriceTag)
// ============================================================================

impl PaygateProtocol for v2::PaymentRequirements {
    type PaymentPayload = v2::PaymentPayload<v2::PaymentRequirements, serde_json::Value>;

    const PAYMENT_HEADER_NAME: &'static str = "Payment-Signature";

    fn make_verify_request(
        payment_payload: Self::PaymentPayload,
        accepts: &[Self],
        _resource: &v2::ResourceInfo,
    ) -> Result<proto::VerifyRequest, VerificationError> {
        // In V2, the accepted requirements are embedded in the payload
        // Resource info is already included in the payment payload from the client
        let accepted = &payment_payload.accepted;

        // Find matching requirements from our accepts list
        let selected = accepts
            .iter()
            .find(|requirement| {
                requirement.scheme == accepted.scheme && requirement.network == accepted.network
            })
            .ok_or(VerificationError::NoPaymentMatching)?;

        // Build the V2 verify request
        let verify_request = v2::VerifyRequest {
            x402_version: v2::X402Version2,
            payment_payload,
            payment_requirements: selected.clone(),
        };

        let json = serde_json::to_value(&verify_request)
            .map_err(|e| VerificationError::VerificationFailed(format!("{e}")))?;

        Ok(proto::VerifyRequest::from(json))
    }

    fn error_into_response(
        err: PaygateError,
        accepts: &[Self],
        resource: &v2::ResourceInfo,
    ) -> Response {
        match err {
            PaygateError::Verification(err) => {
                let payment_required_response = v2::PaymentRequired {
                    error: Some(err.to_string()),
                    accepts: accepts.to_vec(),
                    x402_version: v2::X402Version2,
                    resource: resource.clone(),
                };
                // V2 sends payment required in the "Payment-Required" header (base64 encoded)
                let payment_required_bytes =
                    serde_json::to_vec(&payment_required_response).expect("serialization failed");
                let payment_required_header = Base64Bytes::encode(&payment_required_bytes);
                let header_value = HeaderValue::from_bytes(payment_required_header.as_ref())
                    .expect("Failed to create header value");

                Response::builder()
                    .status(StatusCode::PAYMENT_REQUIRED)
                    .header("Payment-Required", header_value)
                    .body(Body::empty())
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

    fn validate_verify_response(
        verify_response: proto::VerifyResponse,
    ) -> Result<(), VerificationError> {
        let verify_response_v2: v2::VerifyResponse = verify_response
            .try_into()
            .map_err(|e| VerificationError::VerificationFailed(format!("{e}")))?;

        match verify_response_v2 {
            v2::VerifyResponse::Valid { .. } => Ok(()),
            v2::VerifyResponse::Invalid { reason, .. } => {
                Err(VerificationError::VerificationFailed(reason))
            }
        }
    }

    fn enrich_with_capabilities(price_tag: &Self, capabilities: &SupportedResponse) -> Self {
        let mut enriched = price_tag.clone();

        // Only enrich if extra is None (not already set)
        if enriched.extra.is_some() {
            return enriched;
        }

        // Find fee_payer for this network from capabilities.signers
        // V2 uses ChainId directly for network
        if let Some(signers) = capabilities.signers.get(&price_tag.network)
            && let Some(fee_payer) = signers.first()
        {
            let extra = serde_json::json!({ "feePayer": fee_payer });
            enriched.extra = serde_json::to_string(&extra)
                .ok()
                .and_then(|s| serde_json::value::RawValue::from_string(s).ok());
        }

        enriched
    }
}

// ============================================================================
// Unified Paygate Implementation
// ============================================================================

/// Unified payment gate that works with both V1 and V2 protocols.
///
/// The protocol version is determined by the price tag type parameter `P`, which must
/// implement [`PaygateProtocol`]. Use `V1PriceTag` for V1 protocol or `V2PriceTag`
/// (alias for `v2::PaymentRequirements`) for V2 protocol.
pub struct Paygate<P, TFacilitator> {
    /// The facilitator for verifying and settling payments
    pub facilitator: TFacilitator,
    /// Whether to settle before or after request execution
    pub settle_before_execution: bool,
    /// Accepted payment requirements
    pub accepts: Arc<Vec<P>>,
    /// Resource information for the protected endpoint
    pub resource: v2::ResourceInfo,
}

impl<P, TFacilitator> Paygate<P, TFacilitator>
where
    P: PaygateProtocol,
{
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

impl<P, TFacilitator> Paygate<P, TFacilitator>
where
    P: PaygateProtocol,
    TFacilitator: Facilitator,
{
    /// Handles an incoming request, processing payment if required.
    ///
    /// Returns 402 response if payment fails.
    /// Otherwise, returns the response from the inner service.
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
            Err(err) => {
                // Get enriched accepts for 402 response
                let enriched_accepts = self.get_enriched_accepts().await;
                Ok(P::error_into_response(
                    err,
                    &enriched_accepts,
                    &self.resource,
                ))
            }
        }
    }

    /// Gets enriched price tags with facilitator capabilities.
    async fn get_enriched_accepts(&self) -> Vec<P> {
        // Try to get capabilities, use empty if fails
        let capabilities = self.facilitator.supported().await.unwrap_or_default();

        self.accepts
            .iter()
            .map(|pt| P::enrich_with_capabilities(pt, &capabilities))
            .collect()
    }

    /// Handles an incoming request, returning errors as `PaygateError`.
    ///
    /// This is the fallible version of `handle_request` that returns an actual error
    /// instead of turning it into 402 Payment Required response.
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
        let header = extract_payment_header(req.headers(), P::PAYMENT_HEADER_NAME).ok_or(
            VerificationError::PaymentHeaderRequired(P::PAYMENT_HEADER_NAME),
        )?;
        let payment_payload = extract_payment_payload::<P::PaymentPayload>(header)
            .ok_or(VerificationError::InvalidPaymentHeader)?;

        let verify_request =
            P::make_verify_request(payment_payload, &self.accepts, &self.resource)?;

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

            P::validate_verify_response(verify_response)?;

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

    /// Verifies a payment with the facilitator.
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

    /// Settles a payment with the facilitator.
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
}

// ============================================================================
// Helper Functions
// ============================================================================

/// Extracts the payment header value from the header map.
fn extract_payment_header<'a>(header_map: &'a HeaderMap, header_name: &'a str) -> Option<&'a [u8]> {
    header_map.get(header_name).map(|h| h.as_bytes())
}

/// Extracts and deserializes the payment payload from base64-encoded header bytes.
fn extract_payment_payload<T>(header_bytes: &[u8]) -> Option<T>
where
    T: serde::de::DeserializeOwned,
{
    let base64 = Base64Bytes::from(header_bytes).decode().ok()?;
    let value = serde_json::from_slice(base64.as_ref()).ok()?;
    Some(value)
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
