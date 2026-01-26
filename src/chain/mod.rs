//! Blockchain-specific types and providers for x402 payment processing.
//!
//! This module provides abstractions for interacting with different blockchain networks
//! in the x402 protocol. It supports two major blockchain families:
//!
//! - **EIP-155 (EVM)**: Ethereum and EVM-compatible chains like Base, Polygon, Avalanche
//! - **Solana**: The Solana blockchain
//!
//! # Architecture
//!
//! The module is organized around the concept of chain providers and chain identifiers:
//!
//! - [`ChainId`] - A CAIP-2 compliant chain identifier (e.g., `eip155:8453` for Base)
//! - [`ChainIdPattern`] - Pattern matching for chain IDs (exact, wildcard, or set)
//! - [`ChainProvider`] - Enum wrapping chain-specific providers
//! - [`ChainRegistry`] - Registry of configured chain providers
//!
//! # Example
//!
//! ```ignore
//! use x402_rs::chain::{ChainId, ChainIdPattern};
//!
//! // Create a specific chain ID
//! let base = ChainId::new("eip155", "8453");
//!
//! // Create a pattern that matches all EVM chains
//! let all_evm = ChainIdPattern::wildcard("eip155");
//! assert!(all_evm.matches(&base));
//!
//! // Create a pattern for specific chains
//! let mainnet_chains = ChainIdPattern::set("eip155", ["1", "8453", "137"].into_iter().map(String::from).collect());
//! assert!(mainnet_chains.matches(&base));
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use x402_eip155::chain as eip155;
use x402_solana::chain as solana;
use x402_types::chain::{ChainId, ChainProviderOps, ChainRegistry, FromConfig};

#[cfg(feature = "aptos")]
pub mod aptos;

use crate::config::{ChainConfig, ChainsConfig};

/// A blockchain provider that can interact with EVM, Solana, or Aptos chains.
///
/// This enum wraps chain-specific providers and provides a unified interface
/// for the facilitator to interact with different blockchain networks.
///
/// # Variants
///
/// - `Eip155` - Provider for EVM-compatible chains (Ethereum, Base, Polygon, etc.)
/// - `Solana` - Provider for the Solana blockchain
/// - `Aptos` - Provider for the Aptos blockchain
#[derive(Debug, Clone)]
pub enum ChainProvider {
    /// EVM chain provider for EIP-155 compatible networks.
    Eip155(Arc<eip155::Eip155ChainProvider>),
    /// Solana chain provider.
    Solana(Arc<solana::SolanaChainProvider>),
    /// Aptos chain provider.
    #[cfg(feature = "aptos")]
    Aptos(Arc<aptos::AptosChainProvider>),
}

/// Creates a new chain provider from configuration.
///
/// This factory method inspects the configuration type and creates the appropriate
/// chain-specific provider (EVM, Solana, or Aptos).
///
/// # Errors
///
/// Returns an error if:
/// - RPC connection fails
/// - Signer configuration is invalid
/// - Required configuration is missing
#[async_trait::async_trait]
impl FromConfig<ChainConfig> for ChainProvider {
    async fn from_config(chains: &ChainConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let provider = match chains {
            ChainConfig::Eip155(config) => {
                let provider = eip155::Eip155ChainProvider::from_config(config).await?;
                ChainProvider::Eip155(Arc::new(provider))
            }
            ChainConfig::Solana(config) => {
                let provider = solana::SolanaChainProvider::from_config(config).await?;
                ChainProvider::Solana(Arc::new(provider))
            }
            #[cfg(feature = "aptos")]
            ChainConfig::Aptos(config) => {
                let provider = aptos::AptosChainProvider::from_config(config).await?;
                ChainProvider::Aptos(Arc::new(provider))
            }
        };
        Ok(provider)
    }
}

impl ChainProviderOps for ChainProvider {
    fn signer_addresses(&self) -> Vec<String> {
        match self {
            ChainProvider::Eip155(provider) => provider.signer_addresses(),
            ChainProvider::Solana(provider) => provider.signer_addresses(),
            #[cfg(feature = "aptos")]
            ChainProvider::Aptos(provider) => provider.signer_addresses(),
        }
    }

    fn chain_id(&self) -> ChainId {
        match self {
            ChainProvider::Eip155(provider) => provider.chain_id(),
            ChainProvider::Solana(provider) => provider.chain_id(),
            #[cfg(feature = "aptos")]
            ChainProvider::Aptos(provider) => provider.chain_id(),
        }
    }
}

/// Creates a new chain registry from configuration.
///
/// Initializes providers for all configured chains. Each chain configuration
/// is processed and a corresponding provider is created and stored.
///
/// # Errors
///
/// Returns an error if any chain provider fails to initialize.
#[async_trait::async_trait]
impl FromConfig<ChainsConfig> for ChainRegistry<ChainProvider> {
    async fn from_config(chains: &ChainsConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let mut providers = HashMap::new();
        for chain in chains.iter() {
            let chain_provider = ChainProvider::from_config(chain).await?;
            providers.insert(chain_provider.chain_id(), chain_provider);
        }
        Ok(Self::new(providers))
    }
}
