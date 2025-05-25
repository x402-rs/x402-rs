//! Network definitions and known token deployments.
//!
//! This module defines supported networks and their chain IDs,
//! and provides statically known USDC deployments per network.

use alloy::primitives::address;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::fmt::{Display, Formatter};
use std::ops::Deref;

use crate::types::{TokenAsset, TokenAssetEip712};

/// Supported Ethereum-compatible networks.
///
/// Used to differentiate between testnet and mainnet environments for the x402 protocol.
#[derive(Debug, Hash, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Network {
    /// Base Sepolia testnet (chain ID 84532).
    #[serde(rename = "base-sepolia")]
    BaseSepolia,
    /// Base mainnet (chain ID 8453).
    #[serde(rename = "base")]
    Base,
}

impl Display for Network {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Network::BaseSepolia => {
                write!(f, "base-sepolia")
            }
            Network::Base => {
                write!(f, "base")
            }
        }
    }
}

impl Network {
    /// Return the numeric chain ID associated with the network.
    pub fn chain_id(&self) -> u64 {
        match self {
            Network::BaseSepolia => 84532,
            Network::Base => 8453,
        }
    }

    /// Return all known [`Network`] variants.
    pub fn variants() -> &'static [Network] {
        &[Network::BaseSepolia, Network::Base]
    }
}

/// Lazily initialized known USDC deployment on Base Sepolia as [`USDCDeployment`].
static USDC_BASE_SEPOLIA: Lazy<USDCDeployment> = Lazy::new(|| {
    USDCDeployment(TokenAsset {
        address: address!("0x036CbD53842c5426634e7929541eC2318f3dCF7e").into(),
        network: Network::BaseSepolia,
        decimals: 6,
        eip712: TokenAssetEip712 {
            name: "USDC".into(),
            version: "2".into(),
        },
    })
});

/// Lazily initialized known USDC deployment on Base mainnet as [`USDCDeployment`].
static USDC_BASE: Lazy<USDCDeployment> = Lazy::new(|| {
    USDCDeployment(TokenAsset {
        address: address!("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913").into(),
        network: Network::Base,
        decimals: 6,
        eip712: TokenAssetEip712 {
            name: "USDC".into(),
            version: "2".into(),
        },
    })
});

/// A known USDC deployment as a wrapper around [`TokenAsset`].
#[derive(Clone, Debug)]
pub struct USDCDeployment(pub TokenAsset);

impl Deref for USDCDeployment {
    type Target = TokenAsset;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<&USDCDeployment> for TokenAsset {
    fn from(deployment: &USDCDeployment) -> Self {
        deployment.0.clone()
    }
}

impl USDCDeployment {
    /// Return the known USDC deployment for the given network.
    ///
    /// Panic if the network is unsupported (not expected in practice).
    pub fn by_network<N: Borrow<Network>>(network: N) -> &'static USDCDeployment {
        match network.borrow() {
            Network::BaseSepolia => &USDC_BASE_SEPOLIA,
            Network::Base => &USDC_BASE,
        }
    }
}
