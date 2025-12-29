use std::convert::Infallible;
use std::sync::Arc;
use axum::extract::Request;
use axum::response::Response;
use tower::{Layer, Service};
use tower::util::BoxCloneSyncService;
use url::Url;
use x402_rs::facilitator::Facilitator;
use x402_rs::proto::{v1, v2};
use crate::x402::facilitator_client::FacilitatorClient;

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
    description: Option<String>,
    mime_type: Option<String>,
}

impl X402<Arc<FacilitatorClient>> {
    pub fn new(url: &str) -> Self {
        let facilitator = FacilitatorClient::try_from(url)
            .expect("Invalid facilitator URL");
        Self {
            facilitator: Arc::new(facilitator),
            description: None,
            mime_type: None,
        }
    }

    pub fn try_new(url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let facilitator = FacilitatorClient::try_from(url)?;
        Ok(Self {
            facilitator: Arc::new(facilitator),
            description: None,
            mime_type: None,
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
where F: Clone{
    /// Sets the description field on all generated payment requirements.
    pub fn with_description(&self, description: &str) -> Self {
        let mut this = self.clone();
        this.description = Some(description.to_string());
        this
    }

    /// Sets the MIME type of the protected resource.
    /// This is exposed as a part of [`PaymentRequirements`] passed to the client.
    pub fn with_mime_type(&self, mime: &str) -> Self {
        let mut this = self.clone();
        this.mime_type = Some(mime.to_string());
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
            description: None,
            mime_type: None,
        }
    }
}

/// Builder for configuring X402 payment layers.
///
/// The type parameter `V` enforces protocol version consistency:
/// - First `.accept()` determines `V`
/// - Subsequent `.accept()` calls must convert to the same `V`
/// - Mixing v1 and v2 schemes fails at compile time
pub struct X402LayerBuilder<TAccept, TFacilitator> {
    facilitator: TFacilitator,
    accepts: Vec<TAccept>,
    description: Option<String>,
    mime_type: Option<String>,
}

impl<TAccept, TFacilitator> X402LayerBuilder<TAccept, TFacilitator> {
    /// Add another payment option.
    ///
    /// The requirement must convert to the same type `V` as the first `.accept()`.
    /// This is enforced at compile time.
    ///
    /// # Arguments
    ///
    /// * `req` - A payment requirement that implements `Into<V>`
    pub fn accept<R: Into<TAccept>>(mut self, req: R) -> Self {
        self.accepts.push(req.into());
        self
    }

    /// Set a description of what the payment grants access to.
    ///
    /// This is included in 402 responses to inform clients what they're paying for.
    pub fn description(mut self, desc: impl Into<String>) -> Self {
        self.description = Some(desc.into());
        self
    }

    /// Set the MIME type of the protected resource.
    ///
    /// Defaults to "application/json" if not specified.
    pub fn mime_type(mut self, mime: impl Into<String>) -> Self {
        self.mime_type = Some(mime.into());
        self
    }
}

impl<S, TAccept, TFacilitator> Layer<S> for X402LayerBuilder<TAccept, TFacilitator>
where
    S: Service<Request, Response = Response, Error = Infallible> + Clone + Send + Sync + 'static,
    S::Future: Send + 'static,
    TFacilitator: Facilitator + Clone,
{
    type Service = X402MiddlewareService<TFacilitator>;

    fn layer(&self, inner: S) -> Self::Service {
        todo!()
        // if self.base_url.is_none() && self.resource.is_none() {
        //     #[cfg(feature = "telemetry")]
        //     tracing::warn!(
        //         "X402Middleware base_url is not configured; defaulting to http://localhost/ for resource resolution"
        //     );
        // }
        // X402MiddlewareService {
        //     facilitator: self.facilitator.clone(),
        //     settle_before_execution: self.settle_before_execution,
        //     inner: BoxCloneSyncService::new(inner),
        // }
    }
}

/// Wraps a cloned inner Axum service and augments it with payment enforcement logic.
#[derive(Clone, Debug)]
pub struct X402MiddlewareService<F> {
    /// Payment facilitator (local or remote)
    facilitator: Arc<F>,
    /// Whether to settle payment before executing the request (true) or after (false)
    settle_before_execution: bool,
    /// The inner Axum service being wrapped
    inner: BoxCloneSyncService<Request, Response, Infallible>,
}
