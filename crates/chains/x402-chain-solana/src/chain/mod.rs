//! Solana chain support for x402 payments.
//!
//! This module provides types and providers for interacting with the Solana blockchain
//! in the x402 protocol. It supports SPL token transfers for payment settlement.
//!
//! # Key Types
//!
//! - [`SolanaChainReference`] - A 32-character genesis hash identifying a Solana network
//! - [`SolanaChainProvider`] - Provider for interacting with Solana chains
//! - [`SolanaTokenDeployment`] - Token deployment information including mint address and decimals
//! - [`Address`] - A Solana public key (base58-encoded)
//!
//! # Solana Networks
//!
//! Solana networks are identified by the first 32 characters of their genesis block hash:
//! - Mainnet: `5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp`
//! - Devnet: `EtWTRABZaYq6iMfeYKouRu166VU2xqa1`
//!
//! # Example
//!
//! ```ignore
//! use x402_rs::chain::solana::{SolanaChainReference, SolanaTokenDeployment};
//! use x402_rs::networks::{KnownNetworkSolana, USDC};
//!
//! // Get USDC deployment on Solana mainnet
//! let usdc = USDC::solana();
//! assert_eq!(usdc.decimals, 6);
//!
//! // Parse a human-readable amount
//! let amount = usdc.parse("10.50").unwrap();
//! // amount.amount is now 10_500_000 (10.50 * 10^6)
//! ```

pub mod types;
pub use types::*;

#[cfg(feature = "facilitator")]
pub mod provider;
#[cfg(feature = "facilitator")]
pub use provider::*;
#[cfg(feature = "facilitator")]
pub mod config;

#[cfg(any(feature = "facilitator", feature = "client"))]
pub mod rpc;
