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
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller,
};
use alloy::providers::{Identity, ProviderBuilder, RootProvider};
use alloy::signers::local::PrivateKeySigner;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::collections::HashMap;
use std::env;

use crate::network::Network;

/// The fully composed Ethereum provider type used in this project.
///
/// Combines multiple filler layers for gas, nonce, chain ID, blob gas, and wallet signing,
/// and wraps a [`RootProvider`] for actual JSON-RPC communication.
pub type EthereumProvider = FillProvider<
    JoinFill<
        JoinFill<
            Identity,
            JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
        >,
        WalletFiller<EthereumWallet>,
    >,
    RootProvider,
>;

const ENV_SIGNER_TYPE: &str = "SIGNER_TYPE";
const ENV_PRIVATE_KEY: &str = "PRIVATE_KEY";
const ENV_RPC_BASE: &str = "RPC_URL_BASE";
const ENV_RPC_BASE_SEPOLIA: &str = "RPC_URL_BASE_SEPOLIA";
const ENV_RPC_XDC: &str = "RPC_URL_XDC";
const ENV_RPC_AVALANCHE_FUJI: &str = "RPC_URL_AVALANCHE_FUJI";
const ENV_RPC_AVALANCHE: &str = "RPC_URL_AVALANCHE";

/// A cache of pre-initialized [`EthereumProvider`] instances keyed by network.
///
/// This struct is responsible for lazily connecting to all configured RPC URLs
/// and wrapping them with appropriate signing and filler middleware.
///
/// Use [`ProviderCache::from_env`] to load credentials and connect using environment variables.
#[derive(Clone, Debug)]
pub struct ProviderCache {
    providers: HashMap<Network, EthereumProvider>,
    eip1559: HashMap<Network, bool>,
}

/// A generic cache of pre-initialized Ethereum provider instances [`ProviderMap::Value`] keyed by network.
///
/// This allows querying configured providers by network, and checking whether the network
/// supports EIP-1559 fee mechanics.
pub trait ProviderMap {
    type Value;

    /// Returns the Ethereum provider for the specified network, if configured.
    fn by_network<N: Borrow<Network>>(&self, network: N) -> Option<&Self::Value>;

    /// Returns `true` if the specified network supports EIP-1559-style transactions.
    fn eip1559<N: Borrow<Network>>(&self, network: N) -> bool;
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
        let mut eip1559 = HashMap::new();
        let wallet = SignerType::from_env()?.make_wallet()?;
        tracing::info!("Using address: {}", wallet.default_signer().address());

        for network in Network::variants() {
            let env_var = match network {
                Network::BaseSepolia => ENV_RPC_BASE_SEPOLIA,
                Network::Base => ENV_RPC_BASE,
                Network::XdcMainnet => ENV_RPC_XDC,
                Network::AvalancheFuji => ENV_RPC_AVALANCHE_FUJI,
                Network::Avalanche => ENV_RPC_AVALANCHE,
            };
            let is_eip1559 = match network {
                Network::BaseSepolia => true,
                Network::Base => true,
                Network::XdcMainnet => false,
                Network::AvalancheFuji => true,
                Network::Avalanche => true,
            };
            eip1559.insert(*network, is_eip1559);

            let rpc_url = env::var(env_var);
            if let Ok(rpc_url) = rpc_url {
                let provider = ProviderBuilder::new()
                    .wallet(wallet.clone())
                    .connect(&rpc_url)
                    .await
                    .map_err(|e| format!("Failed to connect to {network}: {e}"))?;
                providers.insert(*network, provider);
                tracing::info!("Initialized provider for {} at {}", network, rpc_url);
            } else {
                tracing::warn!("No RPC URL configured for {} (skipped)", network);
            }
        }

        Ok(Self { providers, eip1559 })
    }
}

impl ProviderMap for ProviderCache {
    type Value = EthereumProvider;
    fn by_network<N: Borrow<Network>>(&self, network: N) -> Option<&EthereumProvider> {
        self.providers.get(network.borrow())
    }

    fn eip1559<N: Borrow<Network>>(&self, network: N) -> bool {
        self.eip1559.get(network.borrow()).cloned().unwrap_or(true)
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
    pub fn make_wallet(&self) -> Result<EthereumWallet, Box<dyn std::error::Error>> {
        match self {
            SignerType::PrivateKey => {
                let private_key = env::var(ENV_PRIVATE_KEY)
                    .map_err(|_| format!("env {ENV_PRIVATE_KEY} not set"))?;
                let pk_signer: PrivateKeySigner = private_key.parse()?;
                Ok(EthereumWallet::new(pk_signer))
            }
        }
    }
}
