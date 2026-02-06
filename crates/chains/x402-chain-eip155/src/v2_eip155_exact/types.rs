//! Type definitions for the V2 EIP-155 "exact" payment scheme.
//!
//! This module re-exports types from V1 and defines V2-specific wire format
//! types for ERC-3009 based payments on EVM chains.

use x402_types::proto::v2;

use crate::chain::{AssetTransferMethod, ChecksummedAddress, TokenAmount};

/// Re-export the "exact" scheme identifier from V1 (same for both versions).
pub use crate::v1_eip155_exact::types::ExactScheme;

/// Re-export the EVM payload types from V1 (same structure for both versions).
use crate::v1_eip155_exact::types::ExactEvmPayload;

/// Type alias for V2 verify requests using the exact EVM payment scheme.
pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;

/// Type alias for V2 settle requests (same structure as verify requests).
pub type SettleRequest = VerifyRequest;

/// Type alias for V2 payment payloads with embedded requirements and EVM-specific data.
pub type PaymentPayload = v2::PaymentPayload<PaymentRequirements, ExactEvmPayload>;

/// Type alias for V2 payment requirements with EVM-specific types.
///
/// V2 uses CAIP-2 chain IDs and embeds requirements directly in the payload,
/// unlike V1 which uses network names and separate requirement objects.
pub type PaymentRequirements =
    v2::PaymentRequirements<ExactScheme, TokenAmount, ChecksummedAddress, AssetTransferMethod>;
