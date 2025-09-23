//! Network definitions and known token deployments.
//!
//! This module defines supported networks and their chain IDs,
//! and provides statically known USDC deployments per network.

use crate::types::{MixedAddress, TokenAsset, TokenDeployment, TokenDeploymentEip712};
use alloy::primitives::address;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::borrow::Borrow;
use std::fmt::{Display, Formatter};
use std::ops::Deref;
use std::str::FromStr;

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
    /// XDC mainnet (chain ID 50).
    #[serde(rename = "xdc")]
    XdcMainnet,
    /// Avalanche Fuji testnet (chain ID 43113)
    #[serde(rename = "avalanche-fuji")]
    AvalancheFuji,
    /// Avalanche Mainnet (chain ID 43114)
    #[serde(rename = "avalanche")]
    Avalanche,
    /// Solana Mainnet - Live production environment for deployed applications
    #[serde(rename = "solana")]
    Solana,
    /// Solana Devnet - Testing with public accessibility for developers experimenting with their applications
    #[serde(rename = "solana-devnet")]
    SolanaDevnet,
    /// Polygon Amoy testnet (chain ID 80002).
    #[serde(rename = "polygon-amoy")]
    PolygonAmoy,
    /// Polygon mainnet (chain ID 137).
    #[serde(rename = "polygon")]
    Polygon,
}

impl Display for Network {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Network::BaseSepolia => write!(f, "base-sepolia"),
            Network::Base => write!(f, "base"),
            Network::XdcMainnet => write!(f, "xdc"),
            Network::AvalancheFuji => write!(f, "avalanche-fuji"),
            Network::Avalanche => write!(f, "avalanche"),
            Network::Solana => write!(f, "solana"),
            Network::SolanaDevnet => write!(f, "solana-devnet"),
            Network::PolygonAmoy => write!(f, "polygon-amoy"),
            Network::Polygon => write!(f, "polygon"),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum NetworkFamily {
    Evm,
    Solana,
}

impl From<Network> for NetworkFamily {
    fn from(value: Network) -> Self {
        match value {
            Network::BaseSepolia => NetworkFamily::Evm,
            Network::Base => NetworkFamily::Evm,
            Network::XdcMainnet => NetworkFamily::Evm,
            Network::AvalancheFuji => NetworkFamily::Evm,
            Network::Avalanche => NetworkFamily::Evm,
            Network::Solana => NetworkFamily::Solana,
            Network::SolanaDevnet => NetworkFamily::Solana,
            Network::PolygonAmoy => NetworkFamily::Evm,
            Network::Polygon => NetworkFamily::Evm,
        }
    }
}

impl Network {
    /// Return all known [`Network`] variants.
    pub fn variants() -> &'static [Network] {
        &[
            Network::BaseSepolia,
            Network::Base,
            Network::XdcMainnet,
            Network::AvalancheFuji,
            Network::Avalanche,
            Network::Solana,
            Network::SolanaDevnet,
            Network::PolygonAmoy,
            Network::Polygon,
        ]
    }
}

/// Lazily initialized known USDC deployment on Base Sepolia as [`USDCDeployment`].
static USDC_BASE_SEPOLIA: Lazy<USDCDeployment> = Lazy::new(|| {
    USDCDeployment(TokenDeployment {
        asset: TokenAsset {
            address: address!("0x036CbD53842c5426634e7929541eC2318f3dCF7e").into(),
            network: Network::BaseSepolia,
        },
        decimals: 6,
        eip712: Some(TokenDeploymentEip712 {
            name: "USDC".into(),
            version: "2".into(),
        }),
    })
});

/// Lazily initialized known USDC deployment on Base mainnet as [`USDCDeployment`].
static USDC_BASE: Lazy<USDCDeployment> = Lazy::new(|| {
    USDCDeployment(TokenDeployment {
        asset: TokenAsset {
            address: address!("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913").into(),
            network: Network::Base,
        },
        decimals: 6,
        eip712: Some(TokenDeploymentEip712 {
            name: "USD Coin".into(),
            version: "2".into(),
        }),
    })
});

/// Lazily initialized known USDC deployment on XDC mainnet as [`USDCDeployment`].
static USDC_XDC: Lazy<USDCDeployment> = Lazy::new(|| {
    USDCDeployment(TokenDeployment {
        asset: TokenAsset {
            address: address!("0x2A8E898b6242355c290E1f4Fc966b8788729A4D4").into(),
            network: Network::XdcMainnet,
        },
        decimals: 6,
        eip712: Some(TokenDeploymentEip712 {
            name: "Bridged USDC(XDC)".into(),
            version: "2".into(),
        }),
    })
});

/// Lazily initialized known USDC deployment on Avalanche Fuji testnet as [`USDCDeployment`].
static USDC_AVALANCHE_FUJI: Lazy<USDCDeployment> = Lazy::new(|| {
    USDCDeployment(TokenDeployment {
        asset: TokenAsset {
            address: address!("0x5425890298aed601595a70AB815c96711a31Bc65").into(),
            network: Network::AvalancheFuji,
        },
        decimals: 6,
        eip712: Some(TokenDeploymentEip712 {
            name: "USD Coin".into(),
            version: "2".into(),
        }),
    })
});

/// Lazily initialized known USDC deployment on Avalanche Fuji testnet as [`USDCDeployment`].
static USDC_AVALANCHE: Lazy<USDCDeployment> = Lazy::new(|| {
    USDCDeployment(TokenDeployment {
        asset: TokenAsset {
            address: address!("0xB97EF9Ef8734C71904D8002F8b6Bc66Dd9c48a6E").into(),
            network: Network::Avalanche,
        },
        decimals: 6,
        eip712: Some(TokenDeploymentEip712 {
            name: "USD Coin".into(),
            version: "2".into(),
        }),
    })
});

/// Lazily initialized known USDC deployment on Solana mainnet as [`USDCDeployment`].
static USDC_SOLANA: Lazy<USDCDeployment> = Lazy::new(|| {
    USDCDeployment(TokenDeployment {
        asset: TokenAsset {
            address: MixedAddress::Solana(
                Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(),
            ),
            network: Network::Solana,
        },
        decimals: 6,
        eip712: None,
    })
});

/// Lazily initialized known USDC deployment on Solana mainnet as [`USDCDeployment`].
static USDC_SOLANA_DEVNET: Lazy<USDCDeployment> = Lazy::new(|| {
    USDCDeployment(TokenDeployment {
        asset: TokenAsset {
            address: MixedAddress::Solana(
                Pubkey::from_str("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU").unwrap(),
            ),
            network: Network::SolanaDevnet,
        },
        decimals: 6,
        eip712: None,
    })
});

/// Lazily initialized known USDC deployment on Polygon Amoy testnet as [`USDCDeployment`].
static USDC_POLYGON_AMOY: Lazy<USDCDeployment> = Lazy::new(|| {
    USDCDeployment(TokenDeployment {
        asset: TokenAsset {
            address: address!("0x41E94Eb019C0762f9Bfcf9Fb1E58725BfB0e7582").into(),
            network: Network::PolygonAmoy,
        },
        decimals: 6,
        eip712: Some(TokenDeploymentEip712 {
            name: "USDC".into(),
            version: "2".into(),
        }),
    })
});

/// Lazily initialized known USDC deployment on Polygon mainnet as [`USDCDeployment`].
static USDC_POLYGON: Lazy<USDCDeployment> = Lazy::new(|| {
    USDCDeployment(TokenDeployment {
        asset: TokenAsset {
            address: address!("0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359").into(),
            network: Network::Polygon,
        },
        decimals: 6,
        eip712: Some(TokenDeploymentEip712 {
            name: "USDC".into(),
            version: "1".into(),
        }),
    })
});

/// A known USDC deployment as a wrapper around [`TokenDeployment`].
#[derive(Clone, Debug)]
pub struct USDCDeployment(pub TokenDeployment);

impl Deref for USDCDeployment {
    type Target = TokenDeployment;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl From<&USDCDeployment> for TokenDeployment {
    fn from(deployment: &USDCDeployment) -> Self {
        deployment.0.clone()
    }
}

impl From<USDCDeployment> for Vec<TokenAsset> {
    fn from(deployment: USDCDeployment) -> Self {
        vec![deployment.asset.clone()]
    }
}

impl From<&USDCDeployment> for Vec<TokenAsset> {
    fn from(deployment: &USDCDeployment) -> Self {
        vec![deployment.asset.clone()]
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
            Network::XdcMainnet => &USDC_XDC,
            Network::AvalancheFuji => &USDC_AVALANCHE_FUJI,
            Network::Avalanche => &USDC_AVALANCHE,
            Network::Solana => &USDC_SOLANA,
            Network::SolanaDevnet => &USDC_SOLANA_DEVNET,
            Network::PolygonAmoy => &USDC_POLYGON_AMOY,
            Network::Polygon => &USDC_POLYGON,
        }
    }
}
