//! Axum middleware for enforcing [x402](https://www.x402.org) payments on protected routes.
//!
//! This middleware validates incoming `X-Payment` headers using a configured x402 facilitator,
//! and settles valid payments before allowing the request to proceed (but after your business logic!).
//!
//! Returns a `402 Payment Required` JSON response if the request lacks a valid payment.
//!
//! ## Example Usage
//!
//! ```rust,no_run
//! use axum::{Router, routing::get, Json};
//! use axum::response::IntoResponse;
//! use http::StatusCode;
//! use serde_json::json;
//! use x402_rs::network::{Network, USDCDeployment};
//! use x402_axum::layer::X402Middleware;
//! use x402_axum::price::IntoPriceTag;
//!
//! let x402 = X402Middleware::try_from("https://facilitator.ukstv.me/").unwrap();
//! let usdc = USDCDeployment::by_network(Network::BaseSepolia)
//!     .pay_to("0xADDRESS");
//!
//! let app: Router = Router::new().route(
//!     "/protected",
//!     get(my_handler).layer(
//!         x402.with_description("Access to /protected")
//!             .with_price_tag(usdc.amount(0.025).unwrap())
//!     ),
//! );
//!
//! async fn my_handler() -> impl IntoResponse {
//!     (StatusCode::OK, Json(json!({ "hello": "world" })))
//! }
//! ```
//!
//! ## Configuration Notes
//!
//! - **[`X402Middleware::with_price_tag`]** sets the assets and amounts accepted for payment.
//! - **[`X402Middleware::with_description`]** and **[`X402Middleware::with_mime_type`]** are optional but help the payer understand what is being paid for.
//! - **[`X402Middleware::with_resource`]** explicitly sets the full URI of the protected resource.
//!   This avoids recomputing [`PaymentRequirements`] on every request and should be preferred when possible.
//! - If `with_resource` is **not** used, the middleware will compute the resource URI dynamically from the request
//!   and a base URL set via **[`X402Middleware::with_base_url`]**.
//! - If no base URL is provided, the default is `http://localhost/` (⚠️ avoid this in production).
//!
//! ## Best Practices (Production)
//!
//! - Use [`X402Middleware::with_resource`] when the full resource URL is known.
//! - Set[`X402Middleware::with_base_url`] to support dynamic resource resolution.
//! - ⚠️ Avoid relying on fallback `resource` value in production.

use axum_core::body::Body;
use axum_core::{
    extract::Request,
    response::{IntoResponse, Response},
};
use http::{HeaderMap, HeaderValue, StatusCode, Uri};
use once_cell::sync::Lazy;
use serde_json::json;
use std::collections::HashSet;
use std::fmt::{Debug, Display};
use std::sync::Arc;
use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};
use tower::util::BoxCloneSyncService;
use tower::{Layer, Service};
use url::Url;
use x402_rs::facilitator::Facilitator;
use x402_rs::network::Network;
use x402_rs::types::{
    Base64Bytes, FacilitatorErrorReason, MixedAddress, PaymentPayload, PaymentRequiredResponse,
    PaymentRequirements, Scheme, SettleRequest, SettleResponse, TokenAmount, VerifyRequest,
    VerifyResponse, X402Version,
};

#[cfg(feature = "telemetry")]
use tracing::{Instrument, Level, instrument};

use crate::facilitator_client::{FacilitatorClient, FacilitatorClientError};
use crate::price::PriceTag;

/// Middleware layer that enforces x402 payment verification and settlement.
///
/// Wraps an Axum service, intercepts incoming HTTP requests, verifies the payment
/// using the configured facilitator, and performs settlement after a successful response.
/// Adds a `X-Payment-Response` header to the final HTTP response.
#[derive(Clone, Debug)]
pub struct X402Middleware<F> {
    /// The facilitator used to verify and settle payments.
    facilitator: Arc<F>,
    /// Optional description string passed along with payment requirements. Empty string by default.
    description: Option<String>,
    /// Optional MIME type of the protected resource. `application/json` by default.
    mime_type: Option<String>,
    /// Optional resource URL. If not set, it will be derived from a request URI.
    resource: Option<Url>,
    /// Optional base URL for computing full resource URLs if `resource` is not set, see [`X402Middleware::resource`].
    base_url: Option<Url>,
    /// List of price tags accepted for this endpoint.
    price_tag: Vec<PriceTag>,
    /// Timeout in seconds for payment settlement.
    max_timeout_seconds: u64,
    /// Cached set of payment offers for this middleware instance.
    ///
    /// This field holds either:
    /// - a fully constructed list of [`PaymentRequirements`] (if [`X402Middleware::with_resource`] was used),
    /// - or a partial list without `resource`, in which case the resource URL will be computed dynamically per request.
    ///   In this case, please add `base_url` via [`X402Middleware::with_base_url`].
    payment_offers: Arc<PaymentOffers>,
}

impl TryFrom<&str> for X402Middleware<FacilitatorClient> {
    type Error = FacilitatorClientError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        let facilitator = FacilitatorClient::try_from(value)?;
        Ok(X402Middleware::new(facilitator))
    }
}

impl TryFrom<String> for X402Middleware<FacilitatorClient> {
    type Error = FacilitatorClientError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        X402Middleware::try_from(value.as_str())
    }
}

impl<F> X402Middleware<F>
where
    F: Clone,
{
    pub fn new(facilitator: F) -> Self {
        Self {
            facilitator: Arc::new(facilitator),
            description: None,
            mime_type: None,
            resource: None,
            base_url: None,
            max_timeout_seconds: 300,
            price_tag: Vec::new(),
            payment_offers: Arc::new(PaymentOffers::Ready(Arc::new(Vec::new()))),
        }
    }

    pub fn base_url(&self) -> Url {
        self.base_url
            .clone()
            .unwrap_or(Url::parse("http://localhost/").unwrap())
    }

    /// Sets the description field on all generated payment requirements.
    pub fn with_description(&self, description: &str) -> Self {
        let mut this = self.clone();
        this.description = Some(description.to_string());
        this.recompute_offers()
    }

    /// Sets the MIME type of the protected resource.
    /// This is exposed as a part of [`PaymentRequirements`] passed to the client.
    pub fn with_mime_type(&self, mime: &str) -> Self {
        let mut this = self.clone();
        this.mime_type = Some(mime.to_string());
        this.recompute_offers()
    }

    /// Sets the resource URL directly, avoiding fragile auto-detection from the request.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn with_resource(&self, resource: Url) -> Self {
        let mut this = self.clone();
        this.resource = Some(resource);
        this.recompute_offers()
    }

    /// Sets the base URL used to construct resource URLs dynamically.
    ///
    /// Note: If [`with_resource`] is not called, this base URL is combined with
    /// each request's path/query to compute the resource. If not set, defaults to `http://localhost/`.
    ///
    /// ⚠️ In production, prefer calling `with_resource` or setting a precise `base_url` to avoid accidental localhost fallback.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn with_base_url(&self, base_url: Url) -> Self {
        let mut this = self.clone();
        this.base_url = Some(base_url);
        this.recompute_offers()
    }

    /// Sets the maximum allowed payment timeout, in seconds.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn with_max_timeout_seconds(&self, seconds: u64) -> Self {
        let mut this = self.clone();
        this.max_timeout_seconds = seconds;
        this.recompute_offers()
    }

    /// Replaces all price tags with the provided value(s).
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn with_price_tag<T: Into<Vec<PriceTag>>>(&self, price_tag: T) -> Self {
        let mut this = self.clone();
        this.price_tag = price_tag.into();
        this.recompute_offers()
    }

    /// Adds new price tags to the existing list, avoiding duplicates.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn or_price_tag<T: Into<Vec<PriceTag>>>(&self, price_tag: T) -> Self {
        let mut this = self.clone();
        let mut seen: HashSet<PriceTag> = this.price_tag.iter().cloned().collect();
        for tag in price_tag.into() {
            if seen.insert(tag.clone()) {
                this.price_tag.push(tag);
            }
        }
        this.recompute_offers()
    }

    fn recompute_offers(mut self) -> Self {
        let base_url = self.base_url();
        let description = self.description.clone().unwrap_or_default();
        let mime_type = self
            .mime_type
            .clone()
            .unwrap_or("application/json".to_string());
        let max_timeout_seconds = self.max_timeout_seconds;
        let payment_offers = if let Some(resource) = self.resource.clone() {
            let payment_requirements = self
                .price_tag
                .iter()
                .map(|price_tag| {
                    let extra = if let Some(eip712) = price_tag.token.eip712.clone() {
                        Some(json!({
                            "name": eip712.name,
                            "version": eip712.version
                        }))
                    } else {
                        None
                    };
                    PaymentRequirements {
                        scheme: Scheme::Exact,
                        network: price_tag.token.network(),
                        max_amount_required: price_tag.amount,
                        resource: resource.clone(),
                        description: description.clone(),
                        mime_type: mime_type.clone(),
                        pay_to: price_tag.pay_to.clone(),
                        max_timeout_seconds,
                        asset: price_tag.token.address(),
                        extra,
                        output_schema: None,
                    }
                })
                .collect::<Vec<_>>();
            PaymentOffers::Ready(Arc::new(payment_requirements))
        } else {
            let no_resource = self
                .price_tag
                .iter()
                .map(|price_tag| {
                    let extra = if let Some(eip712) = price_tag.token.eip712.clone() {
                        Some(json!({
                            "name": eip712.name,
                            "version": eip712.version
                        }))
                    } else {
                        None
                    };
                    PaymentRequirementsNoResource {
                        scheme: Scheme::Exact,
                        network: price_tag.token.network(),
                        max_amount_required: price_tag.amount,
                        description: description.clone(),
                        mime_type: mime_type.clone(),
                        pay_to: price_tag.pay_to.clone(),
                        max_timeout_seconds,
                        asset: price_tag.token.address(),
                        extra,
                        output_schema: None,
                    }
                })
                .collect::<Vec<_>>();
            PaymentOffers::NoResource {
                partial: no_resource,
                base_url,
            }
        };
        self.payment_offers = Arc::new(payment_offers);
        self
    }
}

impl X402Middleware<FacilitatorClient> {
    pub fn facilitator_url(&self) -> &Url {
        self.facilitator.base_url()
    }
}

/// Wraps a cloned inner Axum service and augments it with payment enforcement logic.
#[derive(Clone, Debug)]
pub struct X402MiddlewareService<F> {
    /// Payment facilitator (local or remote)
    facilitator: Arc<F>,
    /// Payment requirements either with static or dynamic resource URLs
    payment_offers: Arc<PaymentOffers>,
    /// The inner Axum service being wrapped
    inner: BoxCloneSyncService<Request, Response, Infallible>,
}

impl<S, F> Layer<S> for X402Middleware<F>
where
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + Sync + 'static,
    S::Future: Send + 'static,
    F: Facilitator + Clone,
{
    type Service = X402MiddlewareService<F>;

    fn layer(&self, inner: S) -> Self::Service {
        if self.base_url.is_none() && self.resource.is_none() {
            #[cfg(feature = "telemetry")]
            tracing::warn!(
                "X402Middleware base_url is not configured; defaulting to http://localhost/ for resource resolution"
            );
        }
        X402MiddlewareService {
            facilitator: self.facilitator.clone(),
            payment_offers: self.payment_offers.clone(),
            inner: BoxCloneSyncService::new(inner),
        }
    }
}

impl<F> Service<Request> for X402MiddlewareService<F>
where
    F: Facilitator + Clone + Send + Sync + 'static,
{
    type Response = Response;
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<Response, Infallible>> + Send>>;

    /// Delegates readiness polling to the wrapped inner service.
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    /// Intercepts the request, injects payment enforcement logic, and forwards to the wrapped service.
    fn call(&mut self, req: Request) -> Self::Future {
        let payment_requirements =
            gather_payment_requirements(self.payment_offers.as_ref(), req.uri());
        let gate = X402Paygate {
            facilitator: self.facilitator.clone(),
            payment_requirements,
        };
        let inner = self.inner.clone();
        Box::pin(gate.call(inner, req))
    }
}

#[derive(Debug)]
/// Wrapper for producing a `402 Payment Required` response with context.
pub struct X402Error(PaymentRequiredResponse);

impl Display for X402Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "402 Payment Required: {}", self.0)
    }
}

static ERR_PAYMENT_HEADER_REQUIRED: Lazy<String> =
    Lazy::new(|| "X-PAYMENT header is required".to_string());
static ERR_INVALID_PAYMENT_HEADER: Lazy<String> =
    Lazy::new(|| "Invalid or malformed payment header".to_string());
static ERR_NO_PAYMENT_MATCHING: Lazy<String> =
    Lazy::new(|| "Unable to find matching payment requirements".to_string());

/// Middleware application error with detailed context.
///
/// Encapsulates a `402 Payment Required` response that can be returned
/// when payment verification or settlement fails.
impl X402Error {
    pub fn payment_header_required(payment_requirements: Vec<PaymentRequirements>) -> Self {
        let payment_required_response = PaymentRequiredResponse {
            error: ERR_PAYMENT_HEADER_REQUIRED.clone(),
            accepts: payment_requirements,
            x402_version: X402Version::V1,
        };
        Self(payment_required_response)
    }

    pub fn invalid_payment_header(payment_requirements: Vec<PaymentRequirements>) -> Self {
        let payment_required_response = PaymentRequiredResponse {
            error: ERR_INVALID_PAYMENT_HEADER.clone(),
            accepts: payment_requirements,
            x402_version: X402Version::V1,
        };
        Self(payment_required_response)
    }

    pub fn no_payment_matching(payment_requirements: Vec<PaymentRequirements>) -> Self {
        let payment_required_response = PaymentRequiredResponse {
            error: ERR_NO_PAYMENT_MATCHING.clone(),
            accepts: payment_requirements,
            x402_version: X402Version::V1,
        };
        Self(payment_required_response)
    }

    pub fn verification_failed<E2: Display>(
        error: E2,
        payment_requirements: Vec<PaymentRequirements>,
    ) -> Self {
        let payment_required_response = PaymentRequiredResponse {
            error: format!("Verification Failed: {error}"),
            accepts: payment_requirements,
            x402_version: X402Version::V1,
        };
        Self(payment_required_response)
    }

    pub fn settlement_failed<E2: Display>(
        error: E2,
        payment_requirements: Vec<PaymentRequirements>,
    ) -> Self {
        let payment_required_response = PaymentRequiredResponse {
            error: format!("Settlement Failed: {error}"),
            accepts: payment_requirements,
            x402_version: X402Version::V1,
        };
        Self(payment_required_response)
    }
}

impl IntoResponse for X402Error {
    fn into_response(self) -> Response {
        let payment_required_response_bytes =
            serde_json::to_vec(&self.0).expect("serialization failed");
        let body = Body::from(payment_required_response_bytes);
        Response::builder()
            .status(StatusCode::PAYMENT_REQUIRED)
            .header("Content-Type", "application/json")
            .body(body)
            .expect("Fail to construct response")
    }
}

/// A service-level helper struct responsible for verifying and settling
/// x402 payments based on request headers and known payment requirements.
pub struct X402Paygate<F> {
    pub facilitator: Arc<F>,
    pub payment_requirements: Arc<Vec<PaymentRequirements>>,
}

impl<F> X402Paygate<F>
where
    F: Facilitator + Clone + Send + Sync,
{
    /// Parses the `X-Payment` header and returns a decoded [`PaymentPayload`], or constructs a 402 error if missing or malformed as [`X402Error`].
    pub async fn extract_payment_payload(
        &self,
        headers: &HeaderMap,
    ) -> Result<PaymentPayload, X402Error> {
        let payment_header = headers.get("X-Payment");
        let supported = self.facilitator.supported().await.map_err(|e| {
            X402Error(PaymentRequiredResponse {
                x402_version: X402Version::V1,
                error: format!("Unable to retrieve supported payment schemes: {e}"),
                accepts: vec![],
            })
        })?;
        match payment_header {
            None => {
                let requirements = self
                    .payment_requirements
                    .as_ref()
                    .iter()
                    .map(|r| {
                        let mut r = r.clone();
                        let network = r.network;
                        let extra = supported
                            .kinds
                            .iter()
                            .find(|s| s.network == network.to_string())
                            .cloned()
                            .and_then(|s| s.extra);
                        if let Some(extra) = extra {
                            r.extra = Some(json!({
                                "feePayer": extra.fee_payer
                            }));
                            r
                        } else {
                            r
                        }
                    })
                    .collect::<Vec<_>>();
                Err(X402Error::payment_header_required(requirements))
            }
            Some(payment_header) => {
                let base64 = Base64Bytes::from(payment_header.as_bytes());
                let payment_payload = PaymentPayload::try_from(base64);
                match payment_payload {
                    Ok(payment_payload) => Ok(payment_payload),
                    Err(_) => Err(X402Error::invalid_payment_header(
                        self.payment_requirements.as_ref().clone(),
                    )),
                }
            }
        }
    }

    /// Finds the payment requirement entry matching the given payload's scheme and network.
    fn find_matching_payment_requirements(
        &self,
        payment_payload: &PaymentPayload,
    ) -> Option<PaymentRequirements> {
        self.payment_requirements
            .iter()
            .find(|requirement| {
                requirement.scheme == payment_payload.scheme
                    && requirement.network == payment_payload.network
            })
            .cloned()
    }

    /// Verifies the provided payment using the facilitator and known requirements. Returns a [`VerifyRequest`] if the payment is valid.
    #[cfg_attr(
        feature = "telemetry",
        instrument(name = "x402.verify_payment", skip_all, err)
    )]
    pub async fn verify_payment(
        &self,
        payment_payload: PaymentPayload,
    ) -> Result<VerifyRequest, X402Error> {
        let selected = self
            .find_matching_payment_requirements(&payment_payload)
            .ok_or(X402Error::no_payment_matching(
                self.payment_requirements.as_ref().clone(),
            ))?;
        let verify_request = VerifyRequest {
            x402_version: payment_payload.x402_version,
            payment_payload,
            payment_requirements: selected,
        };
        let verify_response = self
            .facilitator
            .verify(&verify_request)
            .await
            .map_err(|e| {
                X402Error::verification_failed(e, self.payment_requirements.as_ref().clone())
            })?;
        match verify_response {
            VerifyResponse::Valid { .. } => Ok(verify_request),
            VerifyResponse::Invalid { reason, .. } => Err(X402Error::verification_failed(
                reason,
                self.payment_requirements.as_ref().clone(),
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
        settle_request: &SettleRequest,
    ) -> Result<SettleResponse, X402Error> {
        let settlement = self.facilitator.settle(settle_request).await.map_err(|e| {
            X402Error::settlement_failed(e, self.payment_requirements.as_ref().clone())
        })?;
        if settlement.success {
            Ok(settlement)
        } else {
            let error_reason = settlement
                .error_reason
                .unwrap_or(FacilitatorErrorReason::InvalidScheme);
            Err(X402Error::settlement_failed(
                error_reason,
                self.payment_requirements.as_ref().clone(),
            ))
        }
    }

    /// Processes an incoming request through the middleware:
    /// determines payment requirements, verifies the payment,
    /// and invokes the inner Axum handler if the payment is valid.
    /// Adds a `X-Payment-Response` header to the response on success.
    pub async fn call<
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
    {
        Ok(self.handle_request(inner, req).await)
    }

    /// Orchestrates the full payment lifecycle: verifies the request, calls to the inner handler, and settles the payment, returns proper HTTP response.
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
        mut inner: S,
        req: http::Request<ReqBody>,
    ) -> Response
    where
        S::Response: IntoResponse,
        S::Error: IntoResponse,
    {
        let payment_payload = match self.extract_payment_payload(req.headers()).await {
            Ok(payment_payload) => payment_payload,
            Err(err) => {
                #[cfg(feature = "telemetry")]
                tracing::event!(Level::INFO, status = "failed", "No valid payment provided");
                return err.into_response();
            }
        };
        let verify_request = match self.verify_payment(payment_payload).await {
            Ok(verify_request) => verify_request,
            Err(err) => return err.into_response(),
        };
        let inner_fut = {
            #[cfg(feature = "telemetry")]
            {
                inner.call(req).instrument(tracing::info_span!("inner"))
            }
            #[cfg(not(feature = "telemetry"))]
            {
                inner.call(req)
            }
        };
        let response = match inner_fut.await {
            Ok(response) => response,
            Err(err) => return err.into_response(),
        };
        if response.status().is_client_error() || response.status().is_server_error() {
            return response.into_response();
        }
        let settlement = match self.settle_payment(&verify_request).await {
            Ok(settlement) => settlement,
            Err(err) => return err.into_response(),
        };
        let payment_header: Base64Bytes = match settlement.try_into() {
            Ok(payment_header) => payment_header,
            Err(err) => {
                return X402Error::settlement_failed(
                    err,
                    self.payment_requirements.as_ref().clone(),
                )
                .into_response();
            }
        };
        let header_value = match HeaderValue::from_bytes(payment_header.as_ref()) {
            Ok(header_value) => header_value,
            Err(err) => {
                return X402Error::settlement_failed(
                    err,
                    self.payment_requirements.as_ref().clone(),
                )
                .into_response();
            }
        };
        let mut res = response;
        res.headers_mut().insert("X-Payment-Response", header_value);
        res.into_response()
    }
}

/// A variant of [`PaymentRequirements`] without the `resource` field.
/// This allows resources to be dynamically inferred per request.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PaymentRequirementsNoResource {
    pub scheme: Scheme,
    pub network: Network,
    pub max_amount_required: TokenAmount,
    // no resource: Url,
    pub description: String,
    pub mime_type: String,
    pub pay_to: MixedAddress,
    pub max_timeout_seconds: u64,
    pub asset: MixedAddress,
    pub extra: Option<serde_json::Value>,
    pub output_schema: Option<serde_json::Value>,
}

impl PaymentRequirementsNoResource {
    /// Converts this partial requirement into a full [`PaymentRequirements`]
    /// using the provided resource URL.
    pub fn to_payment_requirements(&self, resource: Url) -> PaymentRequirements {
        PaymentRequirements {
            scheme: self.scheme,
            network: self.network,
            max_amount_required: self.max_amount_required,
            resource,
            description: self.description.clone(),
            mime_type: self.mime_type.clone(),
            pay_to: self.pay_to.clone(),
            max_timeout_seconds: self.max_timeout_seconds,
            asset: self.asset.clone(),
            extra: self.extra.clone(),
            output_schema: self.output_schema.clone(),
        }
    }
}

/// Enum capturing either fully constructed [`PaymentRequirements`] (with `resource`)
/// or resource-less variants that must be completed at runtime.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PaymentOffers {
    /// [`PaymentRequirements`] with static `resource` field.
    Ready(Arc<Vec<PaymentRequirements>>),
    /// [`PaymentRequirements`] lacking `resource`, to be added per request.
    NoResource {
        partial: Vec<PaymentRequirementsNoResource>,
        base_url: Url,
    },
}

/// Constructs a full list of [`PaymentRequirements`] for a request.
///
/// This function returns a shared, reference-counted vector of [`PaymentRequirements`]
/// based on the provided [`PaymentOffers`].
///
/// - If `payment_offers` is [`PaymentOffers::Ready`], it returns an Arc clone of the precomputed requirements.
/// - If `payment_offers` is [`PaymentOffers::NoResource`], it dynamically constructs the `resource` URI
///   by combining the `base_url` with the request's path and query, and completes each
///   partial `PaymentRequirementsNoResource` into a full `PaymentRequirements`.
///
/// # Arguments
///
/// * `payment_offers` - The current payment offer configuration, either precomputed or partial.
/// * `req_uri` - The incoming request URI used to construct the full resource path if needed.
///
/// # Returns
///
/// An `Arc<Vec<PaymentRequirements>>` ready to be passed to a facilitator for verification.
fn gather_payment_requirements(
    payment_offers: &PaymentOffers,
    req_uri: &Uri,
) -> Arc<Vec<PaymentRequirements>> {
    match payment_offers {
        PaymentOffers::Ready(requirements) => {
            // requirements is &Arc<Vec<PaymentRequirements>>
            Arc::clone(requirements)
        }
        PaymentOffers::NoResource { partial, base_url } => {
            let resource = {
                let mut resource_url = base_url.clone();
                resource_url.set_path(req_uri.path());
                resource_url.set_query(req_uri.query());
                resource_url
            };
            let payment_requirements = partial
                .iter()
                .map(|partial| partial.to_payment_requirements(resource.clone()))
                .collect::<Vec<_>>();
            Arc::new(payment_requirements)
        }
    }
}
