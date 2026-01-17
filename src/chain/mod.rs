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
//! # Submodules
//!
//! - [`eip155`] - EVM chain support including transaction signing and ERC-3009 transfers
//! - [`solana`] - Solana chain support including SPL token transfers
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

mod chain_id;
pub mod eip155;
pub mod solana;

pub use chain_id::*;

use crate::config::{ChainConfig, ChainsConfig};
use std::collections::HashMap;
use std::sync::Arc;

/// Trait for extracting a specific provider type from a chain provider.
///
/// This is similar to Axum's `FromRequest` trait - it allows types to
/// define how they can be extracted from a generic chain provider.
///
/// Returns `Option<Self>` - `None` if this provider type cannot be
/// extracted from the given chain provider (e.g., trying to extract
/// an EIP-155 provider from a Solana-only chain provider).
pub trait FromChainProvider<P>: Sized {
    /// Attempt to extract Self from the chain provider.
    /// Returns `None` if extraction is not possible.
    fn from_chain_provider(provider: &P) -> Option<Self>;
}

// FIXME doc comments
#[async_trait::async_trait]
pub trait FromConfig<TConfig>
where
    Self: Sized,
{
    async fn from_config(config: &TConfig) -> Result<Self, Box<dyn std::error::Error>>;
}

/// A blockchain provider that can interact with either EVM or Solana chains.
///
/// This enum wraps chain-specific providers and provides a unified interface
/// for the facilitator to interact with different blockchain networks.
///
/// # Variants
///
/// - `Eip155` - Provider for EVM-compatible chains (Ethereum, Base, Polygon, etc.)
/// - `Solana` - Provider for the Solana blockchain
#[derive(Debug, Clone)]
pub enum ChainProvider {
    /// EVM chain provider for EIP-155 compatible networks.
    Eip155(Arc<eip155::Eip155ChainProvider>),
    /// Solana chain provider.
    Solana(Arc<solana::SolanaChainProvider>),
}

/// Creates a new chain provider from configuration.
///
/// This factory method inspects the configuration type and creates the appropriate
/// chain-specific provider (EVM or Solana).
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
        };
        Ok(provider)
    }
}

/// Common operations available on all chain providers.
///
/// This trait provides a unified interface for querying chain provider metadata
/// regardless of the underlying blockchain type.
pub trait ChainProviderOps {
    /// Returns the addresses of all configured signers for this chain.
    ///
    /// For EVM chains, these are Ethereum addresses (0x-prefixed hex).
    /// For Solana, these are base58-encoded public keys.
    fn signer_addresses(&self) -> Vec<String>;

    /// Returns the CAIP-2 chain identifier for this provider.
    fn chain_id(&self) -> ChainId;
}

impl ChainProviderOps for ChainProvider {
    fn signer_addresses(&self) -> Vec<String> {
        match self {
            ChainProvider::Eip155(provider) => provider.signer_addresses(),
            ChainProvider::Solana(provider) => provider.signer_addresses(),
        }
    }

    fn chain_id(&self) -> ChainId {
        match self {
            ChainProvider::Eip155(provider) => provider.chain_id(),
            ChainProvider::Solana(provider) => provider.chain_id(),
        }
    }
}

/// Registry of configured chain providers indexed by chain ID.
///
/// The registry is built from configuration and provides lookup methods
/// for finding providers by exact chain ID or by pattern matching.
///
/// # Type Parameters
///
/// - `P` - The chain provider type (e.g., [`ChainProvider`] or a custom provider type)
///
/// # Example
///
/// ```ignore
/// use x402_rs::chain::{ChainRegistry, ChainIdPattern, ChainProvider};
/// use x402_rs::config::Config;
///
/// let config = Config::load()?;
/// let registry = ChainRegistry::from_config(config.chains()).await?;
///
/// // Find provider for a specific chain
/// let base_provider = registry.by_chain_id(ChainId::new("eip155", "8453"));
///
/// // Find provider matching a pattern
/// let any_evm = registry.by_chain_id_pattern(&ChainIdPattern::wildcard("eip155"));
/// ```
#[derive(Debug)]
pub struct ChainRegistry<P>(HashMap<ChainId, P>);

impl<P> ChainRegistry<P> {
    pub fn new(providers: HashMap<ChainId, P>) -> Self {
        Self(providers)
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

impl<P> ChainRegistry<P> {
    /// Looks up a provider by exact chain ID.
    ///
    /// Returns `None` if no provider is configured for the given chain.
    #[allow(dead_code)]
    pub fn by_chain_id(&self, chain_id: ChainId) -> Option<&P> {
        self.0.get(&chain_id)
    }

    /// Looks up providers by chain ID pattern matching.
    ///
    /// Returns all providers whose chain IDs match the given pattern.
    /// The pattern can be:
    /// - Wildcard: Matches any chain within a namespace (e.g., `eip155:*`)
    /// - Exact: Matches a specific chain (e.g., `eip155:8453`)
    /// - Set: Matches any chain from a set of references (e.g., `eip155:{1,8453,137}`)
    ///
    /// # Example
    ///
    /// ```ignore
    /// use x402_rs::chain::{ChainRegistry, ChainIdPattern};
    /// use x402_rs::config::Config;
    ///
    /// let config = Config::load()?;
    /// let registry = ChainRegistry::from_config(config.chains()).await?;
    ///
    /// // Find all EVM chain providers
    /// let evm_providers = registry.by_chain_id_pattern(&ChainIdPattern::wildcard("eip155"));
    /// assert!(!evm_providers.is_empty());
    ///
    /// // Find providers for specific chains
    /// let mainnet_chains = ChainIdPattern::set("eip155", ["1", "8453", "137"].into_iter().map(String::from).collect());
    /// let mainnet_providers = registry.by_chain_id_pattern(&mainnet_chains);
    /// ```
    pub fn by_chain_id_pattern(&self, pattern: &ChainIdPattern) -> Vec<&P> {
        self.0
            .iter()
            .filter_map(|(chain_id, provider)| pattern.matches(chain_id).then_some(provider))
            .collect()
    }
}

/// A token amount paired with its deployment information.
///
/// This type associates a numeric amount with the token deployment it refers to,
/// enabling type-safe handling of token amounts across different chains and tokens.
///
/// # Type Parameters
///
/// - `TAmount` - The numeric type for the amount (e.g., `U256` for EVM, `u64` for Solana)
/// - `TToken` - The token deployment type containing chain and address information
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct DeployedTokenAmount<TAmount, TToken> {
    /// The token amount in the token's smallest unit (e.g., wei for ETH, lamports for SOL).
    pub amount: TAmount,
    /// The token deployment information including chain, address, and decimals.
    pub token: TToken,
}
