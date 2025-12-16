pub mod v1_eip155_exact;
pub mod v1_solana_exact;

pub use v1_eip155_exact::V1Eip155Exact;

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Debug, Display, Formatter};
use std::ops::Deref;
use std::str::FromStr;

use crate::config::SchemeConfig;
use crate::facilitator_local::FacilitatorLocalError;
use crate::chain::{ChainId, ChainProvider, ChainProviderOps, ChainRegistry};
use crate::proto;
use crate::scheme::v1_solana_exact::V1SolanaExact;

#[async_trait::async_trait]
pub trait X402SchemeHandler: Send + Sync {
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, FacilitatorLocalError>;
    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, FacilitatorLocalError>;
    async fn supported(&self) -> Result<proto::SupportedResponse, FacilitatorLocalError>;
}

pub trait X402SchemeBlueprint {
    fn slug(&self) -> SchemeSlug;
    fn build(
        &self,
        provider: ChainProvider,
    ) -> Result<Box<dyn X402SchemeHandler>, Box<dyn std::error::Error>>;
}

#[derive(Default)]
pub struct SchemeBlueprints(HashMap<SchemeSlug, Box<dyn X402SchemeBlueprint>>);

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
    }

    pub fn and_register<B: X402SchemeBlueprint + 'static>(mut self, blueprint: B) -> Self {
        self.register(blueprint);
        self
    }

    pub fn register<B: X402SchemeBlueprint + 'static>(&mut self, blueprint: B) {
        self.0.insert(blueprint.slug(), Box::new(blueprint));
    }

    pub fn by_slug(&self, slug: &SchemeSlug) -> Option<&dyn X402SchemeBlueprint> {
        self.0.get(slug).map(|v| v.deref())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SchemeSlug {
    x402_version: u8,
    namespace: String,
    name: String,
}

impl SchemeSlug {
    pub fn new<N: Into<String>, M: Into<String>>(x402_version: u8, namespace: N, name: M) -> Self {
        Self {
            x402_version,
            namespace: namespace.into(),
            name: name.into(),
        }
    }
}

impl std::fmt::Display for SchemeSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}:{}:{}", self.x402_version, self.namespace, self.name)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SchemeSlugError {
    #[error("invalid scheme slug format: {0}")]
    InvalidFormat(String),
    #[error("invalid version format: expected 'v<number>', got: {0}")]
    InvalidVersion(String),
}

impl FromStr for SchemeSlug {
    type Err = SchemeSlugError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Expected format: v{version}:{namespace}:{name}
        let parts: Vec<&str> = s.splitn(3, ':').collect();
        if parts.len() != 3 {
            return Err(SchemeSlugError::InvalidFormat(s.to_string()));
        }

        let version_str = parts[0];
        if !version_str.starts_with('v') {
            return Err(SchemeSlugError::InvalidVersion(version_str.to_string()));
        }

        let x402_version: u8 = version_str[1..]
            .parse()
            .map_err(|_| SchemeSlugError::InvalidVersion(version_str.to_string()))?;

        Ok(SchemeSlug {
            x402_version,
            namespace: parts[1].to_string(),
            name: parts[2].to_string(),
        })
    }
}

impl Serialize for SchemeSlug {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SchemeSlug {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        SchemeSlug::from_str(&s).map_err(serde::de::Error::custom)
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
pub struct SchemeRegistry(HashMap<SchemeHandlerSlug, Box<dyn X402SchemeHandler>>);

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
                    config.slug,
                    config.chains
                );
                continue;
            }
            let blueprint = match blueprints.by_slug(&config.slug) {
                Some(blueprint) => blueprint,
                None => {
                    tracing::warn!("No scheme registered: {}", config.slug);
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
            let handler = match blueprint.build(chain_provider) {
                Ok(handler) => handler,
                Err(err) => {
                    tracing::error!("Error building scheme handler for {}: {}", config.slug, err);
                    continue;
                }
            };
            let slug = SchemeHandlerSlug::new(
                chain_id,
                config.slug.x402_version,
                config.slug.name.clone(),
            );
            handlers.insert(slug, handler);
        }
        Self(handlers)
    }

    pub fn by_slug(&self, slug: &SchemeHandlerSlug) -> Option<&dyn X402SchemeHandler> {
        let handler = self.0.get(slug)?.deref();
        Some(handler)
    }

    pub fn values(&self) -> impl Iterator<Item = &dyn X402SchemeHandler> {
        self.0.values().map(|v| v.deref())
    }
}
