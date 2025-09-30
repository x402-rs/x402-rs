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
//! - `PRIVATE_KEY` — the private key used to sign transactions as `"0x..."` string,
//! - `RPC_URL_BASE`, `RPC_URL_BASE_SEPOLIA` — RPC endpoints per network
//!
//! Example usage:
//! ```rust
//! let provider_cache = ProviderCache::from_env().await?;
//! let provider = provider_cache.by_network(Network::Base)?;
//! ```

use alloy::network::EthereumWallet;
use alloy::providers::ProviderBuilder;
use alloy::signers::local::PrivateKeySigner;
use serde::{Deserialize, Serialize};
use solana_sdk::signature::Keypair;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::env;

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
    /// - `PRIVATE_KEY` — the private key used to sign transactions
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
                        let provider = ProviderBuilder::new()
                            .with_simple_nonce_management() // Fetches nonce on every transaction. Better working now. Improve later TODO.
                            .wallet(wallet)
                            .connect(&rpc_url)
                            .await
                            .map_err(|e| format!("Failed to connect to {network}: {e}"))?;
                        let provider = EvmProvider::try_new(provider, is_eip1559, *network)?;
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
    /// A local private key stored in the `PRIVATE_KEY` environment variable.
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
    /// - `PRIVATE_KEY` — the private key used to sign transactions
    pub fn make_evm_wallet(&self) -> Result<EthereumWallet, Box<dyn std::error::Error>> {
        match self {
            SignerType::PrivateKey => {
                let private_key = env::var(ENV_EVM_PRIVATE_KEY)
                    .map_err(|_| format!("env {ENV_EVM_PRIVATE_KEY} not set"))?;
                let pk_signer: PrivateKeySigner = private_key.parse()?;
                Ok(EthereumWallet::new(pk_signer))
            }
        }
    }

    pub fn make_solana_wallet(&self) -> Result<Keypair, Box<dyn std::error::Error>> {
        match self {
            SignerType::PrivateKey => {
                let private_key = env::var(ENV_SOLANA_PRIVATE_KEY)
                    .map_err(|_| format!("env {ENV_EVM_PRIVATE_KEY} not set"))?;
                let keypair = Keypair::from_base58_string(private_key.as_str());
                Ok(keypair)
            }
        }
    }
}
