pub mod v1_eip155_exact;
pub mod v1_solana_exact;
pub mod v2_eip155_exact;
pub mod v2_solana_exact;

pub use v1_eip155_exact::V1Eip155Exact;

use std::collections::HashMap;
use std::fmt::{Debug, Display, Formatter};
use std::ops::Deref;

use crate::chain::{ChainId, ChainProvider, ChainProviderOps, ChainRegistry};
use crate::config::SchemeConfig;
use crate::proto;
use crate::proto::{AsPaymentProblem, ErrorReason, PaymentProblem, PaymentVerificationError};
use crate::scheme::v1_solana_exact::V1SolanaExact;
use crate::scheme::v2_eip155_exact::V2Eip155Exact;
use crate::scheme::v2_solana_exact::V2SolanaExact;

#[async_trait::async_trait]
pub trait X402SchemeFacilitator: Send + Sync {
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError>;
    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, X402SchemeFacilitatorError>;
    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError>;
}

pub trait X402SchemeBlueprint: X402SchemeId + X402SchemeFacilitatorBuilder {}
impl<T> X402SchemeBlueprint for T where T: X402SchemeId + X402SchemeFacilitatorBuilder {}

pub trait X402SchemeId {
    fn x402_version(&self) -> u8 {
        2
    }
    fn namespace(&self) -> &str;
    fn scheme(&self) -> &str;
    fn id(&self) -> String {
        format!(
            "v{}-{}-{}",
            self.x402_version(),
            self.namespace(),
            self.scheme(),
        )
    }
}

pub trait X402SchemeFacilitatorBuilder {
    fn build(
        &self,
        provider: ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>>;
}

#[derive(Debug, thiserror::Error)]
pub enum X402SchemeFacilitatorError {
    #[error(transparent)]
    PaymentVerification(#[from] PaymentVerificationError),
    #[error("Onchain error: {0}")]
    OnchainFailure(String),
}

impl AsPaymentProblem for X402SchemeFacilitatorError {
    fn as_payment_problem(&self) -> PaymentProblem {
        match self {
            X402SchemeFacilitatorError::PaymentVerification(e) => e.as_payment_problem(),
            X402SchemeFacilitatorError::OnchainFailure(e) => {
                PaymentProblem::new(ErrorReason::UnexpectedError, e.to_string())
            }
        }
    }
}

#[derive(Default)]
pub struct SchemeBlueprints(HashMap<String, Box<dyn X402SchemeBlueprint>>);

impl Debug for SchemeBlueprints {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let slugs: Vec<String> = self.0.keys().map(|s| s.to_string()).collect();
        f.debug_tuple("SchemeBlueprints").field(&slugs).finish()
    }
}

impl SchemeBlueprints {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn full() -> Self {
        Self::new()
            .and_register(V1Eip155Exact)
            .and_register(V1SolanaExact)
            .and_register(V2Eip155Exact)
            .and_register(V2SolanaExact)
    }

    pub fn and_register<B: X402SchemeBlueprint + 'static>(mut self, blueprint: B) -> Self {
        self.register(blueprint);
        self
    }

    pub fn register<B: X402SchemeBlueprint + 'static>(&mut self, blueprint: B) {
        self.0.insert(blueprint.id(), Box::new(blueprint));
    }

    pub fn get(&self, id: &str) -> Option<&dyn X402SchemeBlueprint> {
        self.0.get(id).map(|v| v.deref())
    }
}

#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct SchemeHandlerSlug {
    pub chain_id: ChainId,
    pub x402_version: u8,
    pub name: String,
}

impl SchemeHandlerSlug {
    pub fn new(chain_id: ChainId, x402_version: u8, name: String) -> Self {
        Self {
            chain_id,
            x402_version,
            name,
        }
    }
}

impl Display for SchemeHandlerSlug {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}:{}:v{}:{}",
            self.chain_id.namespace, self.chain_id.reference, self.x402_version, self.name
        )
    }
}

#[derive(Default)]
pub struct SchemeRegistry(HashMap<SchemeHandlerSlug, Box<dyn X402SchemeFacilitator>>);

impl Debug for SchemeRegistry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let slugs: Vec<String> = self.0.keys().map(|s| s.to_string()).collect();
        f.debug_tuple("SchemeRegistry").field(&slugs).finish()
    }
}

impl SchemeRegistry {
    pub fn build(
        chains: ChainRegistry,
        blueprints: SchemeBlueprints,
        config: &Vec<SchemeConfig>,
    ) -> Self {
        let mut handlers = HashMap::with_capacity(config.len());
        for config in config {
            if !config.enabled {
                tracing::info!(
                    "Skipping disabled scheme {} for chains {}",
                    config.id,
                    config.chains
                );
                continue;
            }
            let blueprint = match blueprints.get(&config.id) {
                Some(blueprint) => blueprint,
                None => {
                    tracing::warn!("No scheme registered: {}", config.id);
                    continue;
                }
            };
            let chain_provider = match chains.by_chain_id_pattern(&config.chains) {
                Some(chain_provider) => chain_provider,
                None => {
                    tracing::warn!("No chain provider found for {}", config.chains);
                    continue;
                }
            };
            let chain_id = chain_provider.chain_id();
            let handler = match blueprint.build(chain_provider, config.config.clone()) {
                Ok(handler) => handler,
                Err(err) => {
                    tracing::error!("Error building scheme handler for {}: {}", config.id, err);
                    continue;
                }
            };
            let slug = SchemeHandlerSlug::new(
                chain_id.clone(),
                blueprint.x402_version(),
                blueprint.scheme().to_string(),
            );
            tracing::info!(chain_id = %chain_id, scheme = %blueprint.scheme(), id=blueprint.id(), "Registered scheme handler");
            handlers.insert(slug, handler);
        }
        Self(handlers)
    }

    pub fn by_slug(&self, slug: &SchemeHandlerSlug) -> Option<&dyn X402SchemeFacilitator> {
        let handler = self.0.get(slug)?.deref();
        Some(handler)
    }

    pub fn values(&self) -> impl Iterator<Item = &dyn X402SchemeFacilitator> {
        self.0.values().map(|v| v.deref())
    }
}
