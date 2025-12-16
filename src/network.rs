//! Network definitions and known token deployments.
//!
//! This module defines supported networks and their chain IDs,
//! and provides statically known USDC deployments per network.

use serde::{Deserialize, Serialize};
use std::fmt::{Display};
use std::str::FromStr;

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