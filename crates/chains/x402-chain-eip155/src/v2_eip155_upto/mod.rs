//! V2 EIP-155 "upto" payment scheme implementation.
//!
//! This module implements the "upto" payment scheme for EVM chains using
//! the V2 x402 protocol. The upto scheme enables usage-based payments where
//! the client authorizes a maximum amount, and the server settles for the
//! actual amount used at the end of the request.
//!
//! # Key Features
//!
//! - **Variable Settlement**: Client signs a maximum, server settles actual usage
//! - **Permit2 Only**: Uses Permit2 exclusively (EIP-3009 requires exact amounts)
//! - **Zero Settlement**: Supports $0 settlements without on-chain transactions
//! - **Usage-Based Pricing**: Ideal for LLM tokens, bandwidth, compute metering
//!
//! # Differences from Exact Scheme
//!
//! - The `amount` in requirements represents the **maximum** authorized amount
//! - The actual settled amount can be less than or equal to the maximum
//! - Only Permit2 asset transfer method is supported
//! - Settlement can be $0 (no on-chain transaction needed)
//!
//! # Example Use Cases
//!
//! - LLM token generation (charge per token generated)
//! - Bandwidth metering (pay per byte transferred)
//! - Dynamic compute pricing (charge based on resources consumed)
//!
//! # Usage
//!
//! ```ignore
//! use x402_chain_eip155::v2_eip155_upto::V2Eip155Upto;
//! use x402_chain_eip155::networks::{KnownNetworkEip155, USDC};
//!
//! // Create a price tag for up to 5 USDC
//! let usdc = USDC::base();
//! let price = V2Eip155Upto::price_tag(
//!     "0x1234...",  // pay_to address
//!     usdc.amount(5_000_000u64.into()),  // max 5 USDC
//! );
//! ```

#[cfg(feature = "facilitator")]
pub mod facilitator;
#[cfg(feature = "facilitator")]
pub use facilitator::*;

#[cfg(feature = "client")]
pub mod client;
#[cfg(feature = "client")]
pub use client::*;

pub mod types;
pub use types::*;

use x402_types::scheme::X402SchemeId;

/// Scheme identifier for V2 EIP-155 upto payments.
pub struct V2Eip155Upto;

impl X402SchemeId for V2Eip155Upto {
    fn namespace(&self) -> &str {
        "eip155"
    }

    fn scheme(&self) -> &str {
        UptoScheme.as_ref()
    }
}
