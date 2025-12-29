use crate::x402::facilitator_client::FacilitatorClient;
use axum::extract::Request;
use axum::response::Response;
use std::convert::Infallible;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use tower::util::BoxCloneSyncService;
use tower::{Layer, Service};
use url::Url;
use x402_rs::__reexports::alloy_primitives::U256;
use x402_rs::chain::eip155::Eip155TokenDeployment;
use x402_rs::chain::{DeployedTokenAmount, eip155, ChainId};
use x402_rs::facilitator::Facilitator;
use x402_rs::proto::{v1, v2};
use x402_rs::scheme::v1_eip155_exact;

/// The main X402 middleware instance for enforcing x402 payments on routes.
///
/// Create a single instance per application and use it to build payment layers
/// for protected routes.
///
/// **Note**: This implementation is self-contained within the example and will be
/// unbundled into the `x402-axum` library at a later stage.
#[derive(Clone, Debug)]
pub struct X402<F> {
    facilitator: F,
    base_url: Option<Url>,
    /// Whether to settle payment before executing the request (true) or after (false, default).
    settle_before_execution: bool,
    // description: Option<String>,
    // mime_type: Option<String>,
}

impl X402<Arc<FacilitatorClient>> {
    pub fn new(url: &str) -> Self {
        let facilitator = FacilitatorClient::try_from(url).expect("Invalid facilitator URL");
        Self {
            facilitator: Arc::new(facilitator),
            base_url: None,
            settle_before_execution: false,
            // description: None,
            // mime_type: None,
        }
    }

    pub fn try_new(url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let facilitator = FacilitatorClient::try_from(url)?;
        Ok(Self {
            facilitator: Arc::new(facilitator),
            base_url: None,
            settle_before_execution: false,
            // description: None,
            // mime_type: None,
        })
    }

    pub fn facilitator_url(&self) -> &Url {
        self.facilitator.base_url()
    }
}

impl TryFrom<&str> for X402<Arc<FacilitatorClient>> {
    type Error = Box<dyn std::error::Error>;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::try_new(value)
    }
}

impl TryFrom<String> for X402<Arc<FacilitatorClient>> {
    type Error = Box<dyn std::error::Error>;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_new(&value)
    }
}

impl<F> X402<F>
where
    F: Clone,
{
    pub fn with_base_url(&self, base_url: Url) -> X402<F> {
        let mut this = self.clone();
        this.base_url = Some(base_url);
        this
    }

    pub fn settle_before_execution(&self) -> X402<F> {
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

pub trait IntoPriceTag {
    type PriceTag;
    fn into_price_tag(self) -> Self::PriceTag;
}

// impl<TScheme, TAmount, TAddress, TExtra> IntoPriceTag for v1::PaymentRequirements<TScheme, TAmount, TAddress, TExtra>
// where TScheme: ToString, TAmount: ToString, TAddress: ToString, TExtra: serde::Serialize{
//     type PriceTag = v1::PaymentRequirements<String, String, String, serde_json::Value>;
//     fn into_price_tag(self) -> Self::PriceTag {
//         v1::PaymentRequirements {
//             scheme: self.scheme.to_string(),
//             network: self.network,
//     max_amount_required: self.max_amount_required.to_string(),
//     resource: self.resource,
//     description: self.description,
//     mime_type: self.mime_type,
//     output_schema: self.output_schema,
//     pay_to: self.pay_to.to_string(),
//     max_timeout_seconds: self.max_timeout_seconds,
//     asset: self.asset.to_string(),
//     extra: serde_json::to_value(self.extra).ok(),
//         }
//     }
// }

// impl Acceptable for V2PaymentRequirements {
//     type Requirements = V2PaymentRequirements;
//     fn into_requirements(self) -> Self::Requirements { self }
// }

impl<TFacilitator> X402<TFacilitator>
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
            description: None,
            mime_type: None,
            resource: None,
            settle_before_execution: self.settle_before_execution,
        }
    }
}

/// Builder for configuring X402 payment layers.
///
/// The type parameter `V` enforces protocol version consistency:
/// - First `.accept()` determines `V`
/// - Subsequent `.accept()` calls must convert to the same `V`
/// - Mixing v1 and v2 schemes fails at compile time
#[derive(Clone)]
pub struct X402LayerBuilder<TPriceTag, TFacilitator> {
    facilitator: TFacilitator,
    accepts: Vec<TPriceTag>,
    base_url: Option<Url>,
    description: Option<String>,
    mime_type: Option<String>,
    /// Optional resource URL. If not set, it will be derived from a request URI.
    resource: Option<Url>,
    settle_before_execution: bool,
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
    pub fn with_description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set the MIME type of the protected resource.
    ///
    /// Defaults to "application/json" if not specified.
    pub fn with_mime_type(mut self, mime: impl Into<String>) -> Self {
        self.mime_type = Some(mime.into());
        self
    }

    pub fn with_resource(mut self, resource: Url) -> Self {
        self.resource = Some(resource);
        self
    }
}

impl<S, TPriceTag, TFacilitator> Layer<S> for X402LayerBuilder<TPriceTag, TFacilitator>
where
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + Sync + 'static,
    S::Future: Send + 'static,
    TFacilitator: Facilitator + Clone,
{
    type Service = X402MiddlewareService<TFacilitator>;

    fn layer(&self, inner: S) -> Self::Service {
        if self.base_url.is_none() && self.resource.is_none() {
            #[cfg(feature = "telemetry")]
            tracing::warn!(
                "X402Middleware base_url is not configured; defaulting to http://localhost/ for resource resolution"
            );
        }
        X402MiddlewareService {
            facilitator: self.facilitator.clone(),
            settle_before_execution: self.settle_before_execution,
            inner: BoxCloneSyncService::new(inner),
        }
    }
}

/// Wraps a cloned inner Axum service and augments it with payment enforcement logic.
#[derive(Clone, Debug)]
pub struct X402MiddlewareService<F> {
    /// Payment facilitator (local or remote)
    facilitator: F,
    /// Whether to settle payment before executing the request (true) or after (false)
    settle_before_execution: bool,
    /// The inner Axum service being wrapped
    inner: BoxCloneSyncService<Request, Response, Infallible>,
}

#[derive(Debug, Clone)]
pub struct V1Eip155ExactSchemePriceTag {
    pub pay_to: eip155::ChecksummedAddress,
    pub asset: DeployedTokenAmount<U256, Eip155TokenDeployment>,
}

impl IntoPriceTag for V1Eip155ExactSchemePriceTag {
    type PriceTag = V1PriceTag;

    fn into_price_tag(self) -> Self::PriceTag {
        let chain_id: ChainId = self.asset.token.chain_reference.into();
        let network = chain_id.as_network_name().expect(format!("Can not get network name for chain id {}", chain_id).as_str());
        V1PriceTag {
            scheme: "exact".to_string(), // FIXME
            pay_to: self.pay_to.to_string(),
            asset: self.asset.token.address.to_string(),
            network: network.to_string(),
            amount: self.asset.amount.to_string(),
            extra: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct V1PriceTag {
    pub scheme: String,
    pub pay_to: String,
    pub asset: String,
    pub network: String,
    pub amount: String,
    pub extra: Option<serde_json::Value>,
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
        todo!("X402MiddlewareService::call")
        // let offers = self.payment_offers.clone();
        // let facilitator = self.facilitator.clone();
        // let inner = self.inner.clone();
        // let settle_before_execution = self.settle_before_execution;
        // Box::pin(async move {
        //     let payment_requirements =
        //         gather_payment_requirements(offers.as_ref(), req.uri(), req.headers()).await;
        //     let gate = X402Paygate {
        //         facilitator,
        //         payment_requirements,
        //         settle_before_execution,
        //     };
        //     gate.call(inner, req).await
        // })
    }
}
