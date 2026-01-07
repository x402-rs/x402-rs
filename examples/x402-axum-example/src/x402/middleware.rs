use axum::extract::Request;
use axum::response::Response;
use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::util::BoxCloneSyncService;
use tower::{Layer, Service};
use url::Url;
use x402_rs::facilitator::Facilitator;
use x402_rs::proto::server::IntoPriceTag;

use crate::x402::facilitator_client::FacilitatorClient;
use crate::x402::paygate_uni::{Paygate, PaygateProtocol, ResourceInfoBuilder};

/// The main X402 middleware instance for enforcing x402 payments on routes.
///
/// Create a single instance per application and use it to build payment layers
/// for protected routes.
#[derive(Clone, Debug)]
pub struct X402Middleware<F> {
    facilitator: F,
    base_url: Option<Url>,
    settle_before_execution: bool,
}

impl X402Middleware<Arc<FacilitatorClient>> {
    pub fn new(url: &str) -> Self {
        let facilitator = FacilitatorClient::try_from(url).expect("Invalid facilitator URL");
        Self {
            facilitator: Arc::new(facilitator),
            base_url: None,
            settle_before_execution: false,
        }
    }

    pub fn try_new(url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let facilitator = FacilitatorClient::try_from(url)?;
        Ok(Self {
            facilitator: Arc::new(facilitator),
            base_url: None,
            settle_before_execution: false,
        })
    }

    pub fn facilitator_url(&self) -> &Url {
        self.facilitator.base_url()
    }
}

impl TryFrom<&str> for X402Middleware<Arc<FacilitatorClient>> {
    type Error = Box<dyn std::error::Error>;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl TryFrom<String> for X402Middleware<Arc<FacilitatorClient>> {
    type Error = Box<dyn std::error::Error>;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(&value)
    }
}

impl<F> X402Middleware<F>
where
    F: Clone,
{
    pub fn with_base_url(&self, base_url: Url) -> X402Middleware<F> {
        let mut this = self.clone();
        this.base_url = Some(base_url);
        this
    }

    pub fn settle_before_execution(&self) -> X402Middleware<F> {
        let mut this = self.clone();
        this.settle_before_execution = true;
        this
    }

    /// Disables settlement prior to request execution (default behavior).
    ///
    /// When disabled, settlement occurs after successful request execution.
    /// This is the default behavior and allows the application to process
    /// the request before committing the payment on-chain.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn settle_after_execution(&self) -> Self {
        let mut this = self.clone();
        this.settle_before_execution = false;
        this
    }
}

impl<TFacilitator> X402Middleware<TFacilitator>
where
    TFacilitator: Clone,
{
    pub fn with_price_tag<A: IntoPriceTag>(
        &self,
        req: A,
    ) -> X402LayerBuilder<A::PriceTag, TFacilitator> {
        X402LayerBuilder {
            facilitator: self.facilitator.clone(),
            accepts: vec![req.into_price_tag()],
            base_url: self.base_url.clone(),
            resource: ResourceInfoBuilder::default(),
            settle_before_execution: self.settle_before_execution,
        }
    }
}

#[derive(Clone)]
pub struct X402LayerBuilder<TPriceTag, TFacilitator> {
    facilitator: TFacilitator,
    settle_before_execution: bool,
    base_url: Option<Url>,
    accepts: Vec<TPriceTag>,
    resource: ResourceInfoBuilder,
}

impl<TPriceTag, TFacilitator> X402LayerBuilder<TPriceTag, TFacilitator> {
    /// Add another payment option.
    ///
    /// The requirement must convert to the same type `V` as the first `.accept()`.
    /// This is enforced at compile time.
    ///
    /// # Arguments
    ///
    /// * `req` - A payment requirement that implements `Into<V>`
    pub fn with_price_tag<R: Into<TPriceTag>>(mut self, req: R) -> Self {
        self.accepts.push(req.into());
        self
    }

    /// Set a description of what the payment grants access to.
    ///
    /// This is included in 402 responses to inform clients what they're paying for.
    pub fn with_description(mut self, description: String) -> Self {
        self.resource.description = description;
        self
    }

    /// Set the MIME type of the protected resource.
    ///
    /// Defaults to "application/json" if not specified.
    pub fn with_mime_type(mut self, mime: String) -> Self {
        self.resource.mime_type = mime;
        self
    }

    pub fn with_resource(mut self, resource: Url) -> Self {
        self.resource.url = Some(resource.to_string());
        self
    }
}

impl<S, TPriceTag, TFacilitator> Layer<S> for X402LayerBuilder<TPriceTag, TFacilitator>
where
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + Sync + 'static,
    S::Future: Send + 'static,
    TFacilitator: Facilitator + Clone,
    TPriceTag: Clone,
{
    type Service = X402MiddlewareService<TPriceTag, TFacilitator>;

    fn layer(&self, inner: S) -> Self::Service {
        if self.base_url.is_none() && self.resource.url.is_none() {
            #[cfg(feature = "telemetry")]
            tracing::warn!(
                "X402Middleware base_url is not configured; defaulting to http://localhost/ for resource resolution"
            );
        }
        let base_url = self
            .base_url
            .clone()
            .unwrap_or(Url::parse("http://localhost/").expect("Failed to parse default base URL"));
        X402MiddlewareService {
            facilitator: self.facilitator.clone(),
            settle_before_execution: self.settle_before_execution,
            base_url: Arc::new(base_url),
            accepts: Arc::new(self.accepts.clone()),
            resource: Arc::new(self.resource.clone()),
            inner: BoxCloneSyncService::new(inner),
        }
    }
}

/// Wraps a cloned inner Axum service and augments it with payment enforcement logic.
#[derive(Clone, Debug)]
pub struct X402MiddlewareService<TPriceTag, TFacilitator> {
    /// Payment facilitator (local or remote)
    facilitator: TFacilitator,
    base_url: Arc<Url>,
    /// Whether to settle payment before executing the request (true) or after (false)
    settle_before_execution: bool,
    accepts: Arc<Vec<TPriceTag>>,
    resource: Arc<ResourceInfoBuilder>,
    /// The inner Axum service being wrapped
    inner: BoxCloneSyncService<Request, Response, Infallible>,
}

/// Unified Service implementation for any price tag type that implements PaygateProtocol.
///
/// This single implementation replaces the previous separate implementations for
/// V1PriceTag and V2PriceTag, using the unified Paygate from paygate_uni.rs.
impl<TPriceTag, TFacilitator> Service<Request> for X402MiddlewareService<TPriceTag, TFacilitator>
where
    TPriceTag: PaygateProtocol,
    TFacilitator: Facilitator + Clone + Send + Sync + 'static,
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
        let gate = Paygate {
            facilitator: self.facilitator.clone(),
            settle_before_execution: self.settle_before_execution,
            accepts: self.accepts.clone(),
            resource: self.resource.as_resource_info(&self.base_url, req.uri()),
        };
        Box::pin(gate.handle_request(self.inner.clone(), req))
    }
}
