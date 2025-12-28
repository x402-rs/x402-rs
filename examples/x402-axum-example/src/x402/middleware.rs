use std::sync::Arc;

use crate::x402::facilitator_client::FacilitatorClient;

/// The main X402 middleware instance for enforcing x402 payments on routes.
///
/// Create a single instance per application and use it to build payment layers
/// for protected routes.
///
/// **Note**: This implementation is self-contained within the example and will be
/// unbundled into the `x402-axum` library at a later stage.
pub struct X402 {
    facilitator: Arc<FacilitatorClient>,
}

impl X402 {
    /// Create a new X402 instance with a facilitator URL.
    ///
    /// # Arguments
    ///
    /// * `url` - The base URL of the x402 facilitator (e.g., "https://facilitator.x402.rs")
    ///
    /// # Panics
    ///
    /// Panics if the URL is invalid or the facilitator client cannot be created.
    pub fn new(url: &str) -> Self {
        let facilitator = FacilitatorClient::try_from(url)
            .expect("Invalid facilitator URL");
        Self {
            facilitator: Arc::new(facilitator),
        }
    }

    pub fn try_new(url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let facilitator = FacilitatorClient::try_from(url)?;
        Ok(Self {
            facilitator: Arc::new(facilitator),
        })
    }
}

impl X402 {
    /// Start building a payment layer by accepting the first payment option.
    ///
    /// The type of this first option determines the protocol version (v1 or v2).
    /// Subsequent `.accept()` calls must convert to the same type.
    ///
    /// # Arguments
    ///
    /// * `req` - A payment requirement that implements `Into<V>`
    ///
    /// # Example
    ///
    /// ```ignore
    /// let x402 = X402::new("https://facilitator.x402.rs");
    /// let layer = x402.accept(
    ///     V2Eip155Exact::price_tag()
    ///         .pay_to("0xADDRESS")
    ///         .amount(USDC::base_sepolia(), 10000)
    /// )
    /// .description("Weather data");
    /// ```
    pub fn accept<R>(&self, req: R) -> X402LayerBuilder<R> {
        X402LayerBuilder {
            facilitator: self.facilitator.clone(),
            accepts: vec![req],
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
pub struct X402LayerBuilder<V> {
    facilitator: Arc<FacilitatorClient>,
    accepts: Vec<V>,
    description: Option<String>,
    mime_type: Option<String>,
}

impl<V> X402LayerBuilder<V> {
    /// Add another payment option.
    ///
    /// The requirement must convert to the same type `V` as the first `.accept()`.
    /// This is enforced at compile time.
    ///
    /// # Arguments
    ///
    /// * `req` - A payment requirement that implements `Into<V>`
    pub fn accept<R: Into<V>>(mut self, req: R) -> Self {
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
