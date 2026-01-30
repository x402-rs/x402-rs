//! V1 Solana "exact" payment scheme implementation.
//!
//! This module implements the "exact" payment scheme for Solana using
//! the V1 x402 protocol. It uses SPL Token `TransferChecked` instructions
//! for token transfers.
//!
//! # Features
//!
//! - SPL Token and Token-2022 program support
//! - Compute budget instruction validation
//! - Transaction simulation before settlement
//! - Fee payer safety checks
//! - Configurable instruction allowlists/blocklists
//!
//! # Transaction Structure
//!
//! The expected transaction structure is:
//! - Index 0: `SetComputeUnitLimit` instruction
//! - Index 1: `SetComputeUnitPrice` instruction
//! - Index 2: `TransferChecked` instruction (SPL Token or Token-2022)
//! - Index 3+: Additional instructions (if allowed by configuration)
//!
//! # Usage
//!
//! ```ignore
//! use x402::scheme::v1_solana_exact::V1SolanaExact;
//! use x402::networks::{KnownNetworkSolana, USDC};
//!
//! // Create a price tag for 1 USDC on Solana mainnet
//! let usdc = USDC::solana_mainnet();
//! let price = V1SolanaExact::price_tag(
//!     "recipient_pubkey...",  // pay_to address
//!     usdc.amount(1_000_000),  // 1 USDC
//! );
//! ```

pub mod types;
pub use types::*;

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

use x402_types::scheme::X402SchemeId;

pub struct V1SolanaExact;

impl X402SchemeId for V1SolanaExact {
    fn x402_version(&self) -> u8 {
        1
    }

    fn namespace(&self) -> &str {
        "solana"
    }

    fn scheme(&self) -> &str {
        ExactScheme.as_ref()
    }
}
