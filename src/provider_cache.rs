//! Ethereum provider cache and initialization logic.
//!
//! This module defines a cache of configured Ethereum JSON-RPC providers with signing capabilities.
//! Providers are constructed dynamically from environment variables, including private key credentials.
//!
//! This enables interaction with multiple Ethereum-compatible networks using Alloy's `ProviderBuilder`.
//!
//! Supported signer type: `private-key`.
//!
//! Environment variables used:
//! - `SIGNER_TYPE` — currently only `"private-key"` is supported,
//! - `EVM_PRIVATE_KEY` — comma-separated list of private keys used to sign transactions,
//! - `RPC_URL_BASE`, `RPC_URL_BASE_SEPOLIA` — RPC endpoints per network
//!
//! Example usage:
//! ```ignore
//! let provider_cache = ProviderCache::from_env().await?;
//! let provider = provider_cache.by_network(Network::Base)?;
//! ```

use std::borrow::Borrow;
use std::collections::HashMap;

use crate::chain::NetworkProvider;
use crate::config::ChainConfig;
use crate::p1::chain::ChainId;

/// A cache of pre-initialized [`EthereumProvider`] instances keyed by network.
///
/// This struct is responsible for lazily connecting to all configured RPC URLs
/// and wrapping them with appropriate signing and filler middleware.
///
/// Use [`ProviderCache::from_config`] to load credentials and connect using environment variables.
pub struct ProviderCache {
    providers: HashMap<ChainId, NetworkProvider>,
}

/// A generic cache of pre-initialized Ethereum provider instances [`ProviderMap::Value`] keyed by network.
///
/// This allows querying configured providers by network, and checking whether the network
/// supports EIP-1559 fee mechanics.
pub trait ProviderMap {
    type Value;

    /// Returns the Ethereum provider for the specified network, if configured.
    fn by_chain_id<N: Borrow<ChainId>>(&self, chain_id: N) -> Option<&Self::Value>;

    /// An iterator visiting all values in arbitrary order.
    fn values(&self) -> impl Iterator<Item = &Self::Value> + Send;
}

impl<'a> IntoIterator for &'a ProviderCache {
    type Item = (&'a ChainId, &'a NetworkProvider);
    type IntoIter = std::collections::hash_map::Iter<'a, ChainId, NetworkProvider>;

    fn into_iter(self) -> Self::IntoIter {
        self.providers.iter()
    }
}

impl ProviderCache {
    pub async fn from_config(
        chains: &Vec<ChainConfig>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut providers = HashMap::new();
        for chain in chains {
            let network_provider = NetworkProvider::from_config(chain).await?;
            providers.insert(network_provider.chain_id(), network_provider);
        }
        Ok(Self { providers })
    }
}

impl ProviderMap for ProviderCache {
    type Value = NetworkProvider;

    fn by_chain_id<N: Borrow<ChainId>>(&self, chain_id: N) -> Option<&Self::Value> {
        self.providers.get(chain_id.borrow())
    }

    fn values(&self) -> impl Iterator<Item = &Self::Value> {
        self.providers.values()
    }
}
