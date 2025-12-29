use std::sync::Arc;
use url::Url;

use crate::x402::facilitator_client::FacilitatorClient;

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
