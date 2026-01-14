//! Payment scheme implementations for x402.
//!
//! This module provides the extensible scheme system that allows different
//! payment methods to be plugged into the x402 protocol. Each scheme defines
//! how payments are authorized, verified, and settled.
//!
//! # Architecture
//!
//! The scheme system has three main components:
//!
//! 1. **Blueprints** ([`SchemeBlueprints`]) - Factories that create scheme handlers
//! 2. **Handlers** ([`X402SchemeFacilitator`]) - Process verify/settle requests
//! 3. **Registry** ([`SchemeRegistry`]) - Maps chain+scheme combinations to handlers
//!
//! # Built-in Schemes
//!
//! - [`v1_eip155_exact`] - V1 protocol, EVM chains, exact amount transfers
//! - [`v1_solana_exact`] - V1 protocol, Solana, exact amount transfers
//! - [`v2_eip155_exact`] - V2 protocol, EVM chains, exact amount transfers
//! - [`v2_solana_exact`] - V2 protocol, Solana, exact amount transfers
//!
//! # Implementing a Custom Scheme
//!
//! To implement a custom scheme:
//!
//! 1. Implement [`X402SchemeId`] to identify your scheme
//! 2. Implement [`X402SchemeFacilitatorBuilder`] to create handlers
//! 3. Implement [`X402SchemeFacilitator`] for the actual verification/settlement logic
//! 4. Register your scheme with [`SchemeBlueprints::register`]
//!
//! See the [how-to-write-a-scheme](../../docs/how-to-write-a-scheme.md) guide for details.

pub mod v1_eip155_exact;
pub mod v1_solana_exact;
pub mod v2_eip155_exact;
pub mod v2_solana_exact;

pub mod client;

use std::collections::HashMap;
use std::fmt::{Debug, Display, Formatter};
use std::ops::Deref;

use crate::chain::{ChainId, ChainProvider, ChainProviderOps, ChainRegistry};
use crate::config::SchemeConfig;
use crate::proto;
use crate::proto::{AsPaymentProblem, ErrorReason, PaymentProblem, PaymentVerificationError};
use crate::scheme::v1_eip155_exact::V1Eip155Exact;
use crate::scheme::v1_solana_exact::V1SolanaExact;
use crate::scheme::v2_eip155_exact::V2Eip155Exact;
use crate::scheme::v2_solana_exact::V2SolanaExact;

/// Trait for scheme handlers that process payment verification and settlement.
///
/// Implementations of this trait handle the core payment processing logic:
/// verifying that payments are valid and settling them on-chain.
#[async_trait::async_trait]
pub trait X402SchemeFacilitator: Send + Sync {
    /// Verifies a payment authorization without settling it.
    ///
    /// This checks that the payment is properly signed, matches the requirements,
    /// and the payer has sufficient funds.
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError>;

    /// Settles a verified payment on-chain.
    ///
    /// This submits the payment transaction to the blockchain and waits
    /// for confirmation.
    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, X402SchemeFacilitatorError>;

    /// Returns the payment methods supported by this handler.
    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError>;
}

/// Marker trait for types that are both identifiable and buildable.
///
/// This combines [`X402SchemeId`] and [`X402SchemeFacilitatorBuilder`] for
/// use in the blueprint registry.
pub trait X402SchemeBlueprint: X402SchemeId + X402SchemeFacilitatorBuilder {}
impl<T> X402SchemeBlueprint for T where T: X402SchemeId + X402SchemeFacilitatorBuilder {}

/// Trait for identifying a payment scheme.
///
/// Each scheme has a unique identifier composed of the protocol version,
/// chain namespace, and scheme name.
pub trait X402SchemeId {
    /// Returns the x402 protocol version (1 or 2).
    fn x402_version(&self) -> u8 {
        2
    }
    /// Returns the chain namespace (e.g., "eip155", "solana").
    fn namespace(&self) -> &str;
    /// Returns the scheme name (e.g., "exact").
    fn scheme(&self) -> &str;
    /// Returns the full scheme identifier (e.g., "v2-eip155-exact").
    fn id(&self) -> String {
        format!(
            "v{}-{}-{}",
            self.x402_version(),
            self.namespace(),
            self.scheme(),
        )
    }
}

/// Trait for building scheme handlers from chain providers.
pub trait X402SchemeFacilitatorBuilder {
    /// Creates a new scheme handler for the given chain provider.
    ///
    /// # Arguments
    ///
    /// * `provider` - The chain provider to use for on-chain operations
    /// * `config` - Optional scheme-specific configuration
    fn build(
        &self,
        provider: ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>>;
}

/// Errors that can occur during scheme operations.
#[derive(Debug, thiserror::Error)]
pub enum X402SchemeFacilitatorError {
    /// Payment verification failed.
    #[error(transparent)]
    PaymentVerification(#[from] PaymentVerificationError),
    /// On-chain operation failed.
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

/// Registry of scheme blueprints (factories).
///
/// Blueprints are used to create scheme handlers for specific chain providers.
/// Register blueprints at startup, then use them to build handlers.
#[derive(Default)]
pub struct SchemeBlueprints(HashMap<String, Box<dyn X402SchemeBlueprint>>);

impl Debug for SchemeBlueprints {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let slugs: Vec<String> = self.0.keys().map(|s| s.to_string()).collect();
        f.debug_tuple("SchemeBlueprints").field(&slugs).finish()
    }
}

impl SchemeBlueprints {
    /// Creates an empty blueprint registry.
    pub fn new() -> Self {
        Self::default()
    }

    /// Creates a registry with all built-in schemes registered.
    ///
    /// This includes:
    /// - V1 EIP-155 exact
    /// - V1 Solana exact
    /// - V2 EIP-155 exact
    /// - V2 Solana exact
    pub fn full() -> Self {
        Self::new()
            .and_register(V1Eip155Exact)
            .and_register(V1SolanaExact)
            .and_register(V2Eip155Exact)
            .and_register(V2SolanaExact)
    }

    /// Registers a blueprint and returns self for chaining.
    pub fn and_register<B: X402SchemeBlueprint + 'static>(mut self, blueprint: B) -> Self {
        self.register(blueprint);
        self
    }

    /// Registers a scheme blueprint.
    pub fn register<B: X402SchemeBlueprint + 'static>(&mut self, blueprint: B) {
        self.0.insert(blueprint.id(), Box::new(blueprint));
    }

    /// Gets a blueprint by its ID.
    pub fn get(&self, id: &str) -> Option<&dyn X402SchemeBlueprint> {
        self.0.get(id).map(|v| v.deref())
    }
}

/// Unique identifier for a scheme handler instance.
///
/// Combines the chain ID, protocol version, and scheme name to uniquely
/// identify a handler that can process payments for a specific combination.
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
pub struct SchemeHandlerSlug {
    /// The chain this handler operates on.
    pub chain_id: ChainId,
    /// The x402 protocol version.
    pub x402_version: u8,
    /// The scheme name (e.g., "exact").
    pub name: String,
}

impl SchemeHandlerSlug {
    /// Creates a new scheme handler slug.
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

/// Registry of active scheme handlers.
///
/// Maps chain+scheme combinations to their handlers. Built from blueprints
/// and chain providers based on configuration.
#[derive(Default)]
pub struct SchemeRegistry(HashMap<SchemeHandlerSlug, Box<dyn X402SchemeFacilitator>>);

impl Debug for SchemeRegistry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let slugs: Vec<String> = self.0.keys().map(|s| s.to_string()).collect();
        f.debug_tuple("SchemeRegistry").field(&slugs).finish()
    }
}

impl SchemeRegistry {
    /// Builds a scheme registry from blueprints and configuration.
    ///
    /// For each enabled scheme in the config, this finds the matching blueprint
    /// and chain provider, then builds a handler.
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
            let chain_providers = chains.by_chain_id_pattern(&config.chains);
            if chain_providers.is_empty() {
                tracing::warn!("No chain provider found for {}", config.chains);
                continue;
            }

            for chain_provider in chain_providers {
                let chain_id = chain_provider.chain_id();
                let handler = match blueprint.build(chain_provider.clone(), config.config.clone()) {
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
        }
        Self(handlers)
    }

    /// Gets a handler by its slug.
    pub fn by_slug(&self, slug: &SchemeHandlerSlug) -> Option<&dyn X402SchemeFacilitator> {
        let handler = self.0.get(slug)?.deref();
        Some(handler)
    }

    /// Returns an iterator over all registered handlers.
    pub fn values(&self) -> impl Iterator<Item = &dyn X402SchemeFacilitator> {
        self.0.values().map(|v| v.deref())
    }
}
