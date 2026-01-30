//! V1 EIP-155 "exact" payment scheme implementation.
//!
//! This module implements the "exact" payment scheme for EVM chains using
//! the V1 x402 protocol. It uses ERC-3009 `transferWithAuthorization` for
//! gasless token transfers.
//!
//! # Features
//!
//! - EIP-712 typed data signing for payment authorization
//! - EIP-6492 support for counterfactual smart wallet signatures
//! - EIP-1271 support for deployed smart wallet signatures
//! - EOA signature support with split (v, r, s) components
//! - On-chain balance verification before settlement
//!
//! # Signature Handling
//!
//! The facilitator intelligently dispatches to different `transferWithAuthorization`
//! contract functions based on the signature format provided:
//!
//! - **EOA signatures (64-65 bytes)**: Parsed as (r, s, v) components and dispatched to
//!   `transferWithAuthorization(address,address,uint256,uint256,uint256,bytes32,uint8,bytes32,bytes32)`
//!   (the standard EIP-3009 function signature).
//!
//! - **EIP-1271 signatures (any other length)**: Passed as full signature bytes to
//!   `transferWithAuthorization(address,address,uint256,uint256,uint256,bytes32,bytes)`
//!   (a non-standard variant that accepts arbitrary signature bytes for contract wallets).
//!
//! - **EIP-6492 signatures**: Detected by the 32-byte magic suffix and validated via
//!   the universal EIP-6492 validator contract before settlement.
//!
//! # Usage
//!
//! ```ignore
//! use x402::scheme::v1_eip155_exact::V1Eip155Exact;
//! use x402::networks::{KnownNetworkEip155, USDC};
//!
//! // Create a price tag for 1 USDC on Base
//! let usdc = USDC::base();
//! let price = V1Eip155Exact::price_tag(
//!     "0x1234...",  // pay_to address
//!     usdc.amount(1_000_000u64.into()),  // 1 USDC
//! );
//! ```

use x402_types::scheme::X402SchemeId;

#[cfg(feature = "server")]
pub mod server;
#[cfg(feature = "server")]
#[allow(unused_imports)]
pub use server::*;

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

pub struct V1Eip155Exact;

impl X402SchemeId for V1Eip155Exact {
    fn x402_version(&self) -> u8 {
        1
    }
    fn namespace(&self) -> &str {
        "eip155"
    }
    fn scheme(&self) -> &str {
        ExactScheme.as_ref()
    }
}
