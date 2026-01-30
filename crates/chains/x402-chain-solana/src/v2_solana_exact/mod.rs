//! V2 Solana "exact" payment scheme implementation.
//!
//! This module implements the "exact" payment scheme for Solana using
//! the V2 x402 protocol. It builds on the V1 implementation but uses
//! CAIP-2 chain identifiers instead of network names.
//!
//! # Differences from V1
//!
//! - Uses CAIP-2 chain IDs (e.g., `solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp`) instead of network names
//! - Payment requirements are embedded in the payload for verification
//! - Cleaner separation between accepted requirements and authorization
//!
//! # Features
//!
//! - SPL Token and Token-2022 program support
//! - Compute budget instruction validation
//! - Transaction simulation before settlement
//! - Fee payer safety checks
//! - Configurable instruction allowlists/blocklists
//!
//! # Usage
//!
//! ```ignore
//! use x402::scheme::v2_solana_exact::V2SolanaExact;
//! use x402::networks::{KnownNetworkSolana, USDC};
//!
//! // Create a price tag for 1 USDC on Solana mainnet
//! let usdc = USDC::solana_mainnet();
//! let price = V2SolanaExact::price_tag(
//!     "recipient_pubkey...",  // pay_to address
//!     usdc.amount(1_000_000),  // 1 USDC
//! );
//! ```

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "client")]
pub use client::*;

#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
pub use server::*;

#[cfg(feature = "facilitator")]
pub mod facilitator;
#[cfg(feature = "facilitator")]
pub use facilitator::*;

pub mod types;
pub use types::*;

use x402_types::scheme::X402SchemeId;

pub struct V2SolanaExact;

impl X402SchemeId for V2SolanaExact {
    fn namespace(&self) -> &str {
        "solana"
    }

    fn scheme(&self) -> &str {
        ExactScheme.as_ref()
    }
}
