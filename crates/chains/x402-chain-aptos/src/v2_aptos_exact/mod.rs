//! V2 Aptos "exact" payment scheme implementation.
//!
//! This module implements the "exact" payment scheme for Aptos using
//! the V2 x402 protocol. It uses CAIP-2 chain identifiers (aptos:1, aptos:2).
//!
//! # Features
//!
//! - Fungible asset transfers using `0x1::primary_fungible_store::transfer`
//! - Sponsored (gasless) transactions where the facilitator pays gas fees
//! - Transaction simulation before settlement
//! - BCS-encoded transaction validation
//!
//! # Usage
//!
//! ```ignore
//! use x402::scheme::v2_aptos_exact::V2AptosExact;
//! use x402::networks::{KnownNetworkAptos, USDC};
//!
//! // Create a price tag for 1 USDC on Aptos mainnet
//! let usdc = USDC::aptos();
//! let price = V2AptosExact::price_tag(
//!     "0x1234...",  // pay_to address
//!     usdc.amount(1_000_000),  // 1 USDC
//! );
//! ```

#[cfg(feature = "facilitator")]
pub mod facilitator;
#[cfg(feature = "facilitator")]
pub use facilitator::*;

pub mod types;
pub use types::*;

use x402_types::scheme::X402SchemeId;

pub struct V2AptosExact;

impl X402SchemeId for V2AptosExact {
    fn namespace(&self) -> &str {
        "aptos"
    }

    fn scheme(&self) -> &str {
        ExactScheme.as_ref()
    }
}
