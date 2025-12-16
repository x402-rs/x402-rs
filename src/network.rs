//! Network definitions and known token deployments.
//!
//! This module defines supported networks and their chain IDs,
//! and provides statically known USDC deployments per network.

use alloy_primitives::address;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::borrow::Borrow;
use std::fmt::{Display, Formatter};
use std::ops::Deref;
use std::str::FromStr;

use crate::p1::chain::ChainId;
use crate::p1::chain::solana;
use crate::types::{MixedAddress, TokenAsset, TokenDeployment, TokenDeploymentEip712};

/// Supported Ethereum-compatible networks.
///
/// Used to differentiate between testnet and mainnet environments for the x402 protocol.
#[derive(Debug, Hash, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Network {
    // FIXME v1 DELETE NETWORK
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
    /// XRPL EVM mainnet (chain ID 1440000)
    #[serde(rename = "xrpl-evm")]
    XrplEvm,
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
    /// Sei mainnet (chain ID 1329).
    #[serde(rename = "sei")]
    Sei,
    /// Sei testnet (chain ID 1328).
    #[serde(rename = "sei-testnet")]
    SeiTestnet,
}

impl Display for Network {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Network::BaseSepolia => write!(f, "base-sepolia"),
            Network::Base => write!(f, "base"),
            Network::XdcMainnet => write!(f, "xdc"),
            Network::AvalancheFuji => write!(f, "avalanche-fuji"),
            Network::Avalanche => write!(f, "avalanche"),
            Network::XrplEvm => write!(f, "xrpl-evm"),
            Network::Solana => write!(f, "solana"),
            Network::SolanaDevnet => write!(f, "solana-devnet"),
            Network::PolygonAmoy => write!(f, "polygon-amoy"),
            Network::Polygon => write!(f, "polygon"),
            Network::Sei => write!(f, "sei"),
            Network::SeiTestnet => write!(f, "sei-testnet"),
        }
    }
}

impl Into<String> for Network {
    fn into(self) -> String {
        self.to_string()
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
            Network::XrplEvm,
            Network::Solana,
            Network::SolanaDevnet,
            Network::PolygonAmoy,
            Network::Polygon,
            Network::Sei,
            Network::SeiTestnet,
        ]
    }

    pub fn as_chain_id(&self) -> ChainId {
        match self {
            Network::BaseSepolia => ChainId::new("eip155", "84532"),
            Network::Base => ChainId::new("eip155", "8453"),
            Network::XdcMainnet => ChainId::new("eip155", "50"),
            Network::AvalancheFuji => ChainId::new("eip155", "4313"),
            Network::Avalanche => ChainId::new("eip155", "43114"),
            Network::XrplEvm => ChainId::new("eip155", "1440000"),
            Network::PolygonAmoy => ChainId::new("eip155", "80002"),
            Network::Polygon => ChainId::new("eip155", "137"),
            Network::Sei => ChainId::new("eip155", "1329"),
            Network::SeiTestnet => ChainId::new("eip155", "1328"),
            Network::Solana => ChainId::new("solana", "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"),
            Network::SolanaDevnet => ChainId::new("solana", "EtWTRABZaYq6iMfeYKouRu166VU2xqa1"),
        }
    }
}