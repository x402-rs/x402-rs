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

use alloy::network::EthereumWallet;
use alloy::signers::local::PrivateKeySigner;
use serde::{Deserialize, Serialize};
use solana_sdk::signature::Keypair;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::env;
use std::str::FromStr;

use crate::chain::evm::EvmProvider;
use crate::chain::solana::SolanaProvider;
use crate::chain::{NetworkProvider, NetworkProviderOps};
use crate::network::{Network, NetworkFamily};

const ENV_SIGNER_TYPE: &str = "SIGNER_TYPE";
const ENV_EVM_PRIVATE_KEY: &str = "EVM_PRIVATE_KEY";
const ENV_SOLANA_PRIVATE_KEY: &str = "SOLANA_PRIVATE_KEY";
const ENV_RPC_BASE: &str = "RPC_URL_BASE";
const ENV_RPC_BASE_SEPOLIA: &str = "RPC_URL_BASE_SEPOLIA";
const ENV_RPC_XDC: &str = "RPC_URL_XDC";
const ENV_RPC_AVALANCHE_FUJI: &str = "RPC_URL_AVALANCHE_FUJI";
const ENV_RPC_AVALANCHE: &str = "RPC_URL_AVALANCHE";
const ENV_RPC_SOLANA: &str = "RPC_URL_SOLANA";
const ENV_RPC_SOLANA_DEVNET: &str = "RPC_URL_SOLANA_DEVNET";
const ENV_RPC_POLYGON_AMOY: &str = "RPC_URL_POLYGON_AMOY";
const ENV_RPC_POLYGON: &str = "RPC_URL_POLYGON";
const ENV_RPC_SEI: &str = "RPC_URL_SEI";
const ENV_RPC_SEI_TESTNET: &str = "RPC_URL_SEI_TESTNET";

/// A cache of pre-initialized [`EthereumProvider`] instances keyed by network.
///
/// This struct is responsible for lazily connecting to all configured RPC URLs
/// and wrapping them with appropriate signing and filler middleware.
///
/// Use [`ProviderCache::from_env`] to load credentials and connect using environment variables.
#[derive(Clone)]
pub struct ProviderCache {
    providers: HashMap<Network, NetworkProvider>,
}

/// A generic cache of pre-initialized Ethereum provider instances [`ProviderMap::Value`] keyed by network.
///
/// This allows querying configured providers by network, and checking whether the network
/// supports EIP-1559 fee mechanics.
pub trait ProviderMap {
    type Value;

    /// Returns the Ethereum provider for the specified network, if configured.
    fn by_network<N: Borrow<Network>>(&self, network: N) -> Option<&Self::Value>;
}

impl<'a> IntoIterator for &'a ProviderCache {
    type Item = (&'a Network, &'a NetworkProvider);
    type IntoIter = std::collections::hash_map::Iter<'a, Network, NetworkProvider>;

    fn into_iter(self) -> Self::IntoIter {
        self.providers.iter()
    }
}

impl ProviderCache {
    /// Constructs a new [`ProviderCache`] from environment variables.
    ///
    /// Expects the following to be set:
    /// - `SIGNER_TYPE` — currently only `"private-key"` is supported
    /// - `EVM_PRIVATE_KEY` — comma-separated list of private keys used to sign transactions
    /// - `RPC_URL_BASE`, `RPC_URL_BASE_SEPOLIA` — RPC endpoints per network
    ///
    /// Fails if required env vars are missing or if the provider cannot connect.
    pub async fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let mut providers = HashMap::new();
        for network in Network::variants() {
            let env_var = match network {
                Network::BaseSepolia => ENV_RPC_BASE_SEPOLIA,
                Network::Base => ENV_RPC_BASE,
                Network::XdcMainnet => ENV_RPC_XDC,
                Network::AvalancheFuji => ENV_RPC_AVALANCHE_FUJI,
                Network::Avalanche => ENV_RPC_AVALANCHE,
                Network::Solana => ENV_RPC_SOLANA,
                Network::SolanaDevnet => ENV_RPC_SOLANA_DEVNET,
                Network::PolygonAmoy => ENV_RPC_POLYGON_AMOY,
                Network::Polygon => ENV_RPC_POLYGON,
                Network::Sei => ENV_RPC_SEI,
                Network::SeiTestnet => ENV_RPC_SEI_TESTNET,
            };
            let is_eip1559 = match network {
                Network::BaseSepolia => true,
                Network::Base => true,
                Network::XdcMainnet => false,
                Network::AvalancheFuji => true,
                Network::Avalanche => true,
                Network::Solana => false,
                Network::SolanaDevnet => false,
                Network::PolygonAmoy => true,
                Network::Polygon => true,
                Network::Sei => true,
                Network::SeiTestnet => true,
            };

            let rpc_url = env::var(env_var);
            if let Ok(rpc_url) = rpc_url {
                let family: NetworkFamily = (*network).into();
                match family {
                    NetworkFamily::Evm => {
                        let wallet = SignerType::from_env()?.make_evm_wallet()?;
                        let provider =
                            EvmProvider::try_new(wallet, &rpc_url, is_eip1559, *network).await?;
                        let provider = NetworkProvider::Evm(provider);
                        let signer_address = provider.signer_address();
                        providers.insert(*network, provider);
                        tracing::info!(
                            "Initialized provider for {} at {} using {}",
                            network,
                            rpc_url,
                            signer_address
                        );
                    }
                    NetworkFamily::Solana => {
                        let keypair = SignerType::from_env()?.make_solana_wallet()?;
                        let provider = SolanaProvider::try_new(keypair, rpc_url.clone(), *network)?;
                        let provider = NetworkProvider::Solana(provider);
                        let signer_address = provider.signer_address();
                        providers.insert(*network, provider);
                        tracing::info!(
                            "Initialized provider for {} at {} using {}",
                            network,
                            rpc_url,
                            signer_address
                        );
                    }
                }
            } else {
                tracing::warn!("No RPC URL configured for {} (skipped)", network);
            }
        }

        Ok(Self { providers })
    }
}

impl ProviderMap for ProviderCache {
    type Value = NetworkProvider;
    fn by_network<N: Borrow<Network>>(&self, network: N) -> Option<&NetworkProvider> {
        self.providers.get(network.borrow())
    }
}

/// Supported methods for constructing an Ethereum wallet from environment variables.
#[derive(Debug, Hash, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignerType {
    /// A local private key stored in the `EVM_PRIVATE_KEY` environment variable.
    #[serde(rename = "private-key")]
    PrivateKey,
}

impl SignerType {
    /// Parse the signer type from the `SIGNER_TYPE` environment variable.
    fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let signer_type_string =
            env::var(ENV_SIGNER_TYPE).map_err(|_| format!("env {ENV_SIGNER_TYPE} not set"))?;
        match signer_type_string.as_str() {
            "private-key" => Ok(SignerType::PrivateKey),
            _ => Err(format!("Unknown signer type {signer_type_string}").into()),
        }
    }

    /// Constructs an [`EthereumWallet`] based on the [`SignerType`] selected from environment.
    ///
    /// Currently only supports [`SignerType::PrivateKey`] variant, based on the following environment variables:
    /// - `SIGNER_TYPE` — currently only `"private-key"` is supported
    /// - `EVM_PRIVATE_KEY` — comma-separated list of private keys used to sign transactions
    pub fn make_evm_wallet(&self) -> Result<EthereumWallet, Box<dyn std::error::Error>> {
        match self {
            SignerType::PrivateKey => {
                let raw_keys = env::var(ENV_EVM_PRIVATE_KEY)
                    .map_err(|_| format!("env {ENV_EVM_PRIVATE_KEY} not set"))?;
                let keys: Vec<_> = raw_keys
                    .split(',')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(str::to_owned)
                    .collect();
                if keys.is_empty() {
                    return Err("env EVM_PRIVATE_KEY did not contain any private keys".into());
                }

                let mut iter = keys.into_iter();
                let first_key = iter
                    .next()
                    .expect("iterator contains at least one element by construction");
                let first_signer = PrivateKeySigner::from_str(&first_key)
                    .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
                let mut wallet = EthereumWallet::from(first_signer);

                for key in iter {
                    let signer = PrivateKeySigner::from_str(&key)
                        .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
                    wallet.register_signer(signer);
                }

                Ok(wallet)
            }
        }
    }

    pub fn make_solana_wallet(&self) -> Result<Keypair, Box<dyn std::error::Error>> {
        match self {
            SignerType::PrivateKey => {
                let private_key = env::var(ENV_SOLANA_PRIVATE_KEY)
                    .map_err(|_| format!("env {ENV_SOLANA_PRIVATE_KEY} not set"))?;
                let keypair = Keypair::from_base58_string(private_key.as_str());
                Ok(keypair)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::network::{Ethereum as AlloyEthereum, NetworkWallet};
    use alloy::signers::local::PrivateKeySigner;
    use std::str::FromStr;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn restore_env(key: &str, original: Option<String>) {
        if let Some(value) = original {
            // Safety: guarded by `ENV_LOCK`, so no concurrent environment mutation occurs.
            unsafe { env::set_var(key, value) };
        } else {
            // Safety: guarded by `ENV_LOCK`, so no concurrent environment mutation occurs.
            unsafe { env::remove_var(key) };
        }
    }

    #[test]
    fn make_evm_wallet_supports_multiple_private_keys() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        let original_signer_type = env::var(ENV_SIGNER_TYPE).ok();
        let original_evm_keys = env::var(ENV_EVM_PRIVATE_KEY).ok();

        const KEY_1: &str = "0xcafe000000000000000000000000000000000000000000000000000000000001";
        const KEY_2: &str = "0xcafe000000000000000000000000000000000000000000000000000000000002";

        // Safety: guarded by `ENV_LOCK`, so no concurrent environment mutation occurs.
        unsafe {
            env::set_var(ENV_SIGNER_TYPE, "private-key");
            env::set_var(ENV_EVM_PRIVATE_KEY, format!("{KEY_1},{KEY_2}"));
        }

        let signer_type = SignerType::from_env().expect("SIGNER_TYPE");
        let wallet = signer_type
            .make_evm_wallet()
            .expect("wallet constructed from env");

        let expected_primary = PrivateKeySigner::from_str(KEY_1)
            .expect("key1 parses")
            .address();
        let expected_secondary = PrivateKeySigner::from_str(KEY_2)
            .expect("key2 parses")
            .address();

        assert_eq!(
            NetworkWallet::<AlloyEthereum>::default_signer_address(&wallet),
            expected_primary
        );

        let signers: Vec<_> = NetworkWallet::<AlloyEthereum>::signer_addresses(&wallet).collect();
        assert_eq!(signers.len(), 2);
        assert!(signers.contains(&expected_primary));
        assert!(signers.contains(&expected_secondary));

        restore_env(ENV_EVM_PRIVATE_KEY, original_evm_keys);
        restore_env(ENV_SIGNER_TYPE, original_signer_type);
    }
}
