//! Type definitions for the V2 EIP-155 "exact" payment scheme.
//!
//! This module re-exports types from V1 and defines V2-specific wire format
//! types for ERC-3009 based payments on EVM chains.

use alloy_primitives::Bytes;
use serde::{Deserialize, Serialize};
use x402_types::proto::v2;
use x402_types::timestamp::UnixTimestamp;

use crate::chain::{AssetTransferMethod, ChecksummedAddress, DecimalU256, TokenAmount};

/// Re-export the "exact" scheme identifier from V1 (same for both versions).
pub use crate::v1_eip155_exact::types::{ExactEvmPayload as Eip3009Payload, ExactScheme};

/// Type alias for V2 verify requests using the exact EVM payment scheme.
pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;

/// Type alias for V2 settle requests (same structure as verify requests).
pub type SettleRequest = VerifyRequest;

/// Type alias for V2 payment payloads with embedded requirements and EVM-specific data.
pub type PaymentPayload<TPaymentRequirements = PaymentRequirements> =
    v2::PaymentPayload<TPaymentRequirements, ExactEvmPayload>;

/// Type alias for V2 payment requirements with EVM-specific types.
///
/// V2 uses CAIP-2 chain IDs and embeds requirements directly in the payload,
/// unlike V1 which uses network names and separate requirement objects.
pub type PaymentRequirements =
    v2::PaymentRequirements<ExactScheme, TokenAmount, ChecksummedAddress, AssetTransferMethod>;

// FIXME Docs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Authorization {
    pub deadline: UnixTimestamp,
    pub from: ChecksummedAddress,
    pub nonce: DecimalU256,
    pub permitted: Permit2AuthorizationPermitted,
    pub spender: ChecksummedAddress,
    pub witness: Permit2Witness,
}

// FIXME Docs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Witness {
    pub extra: Bytes,
    pub to: ChecksummedAddress,
    pub valid_after: UnixTimestamp,
}

// FIXME Docs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2AuthorizationPermitted {
    pub amount: TokenAmount,
    pub token: ChecksummedAddress,
}

// FIXME Docs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Payload {
    pub permit_2_authorization: Permit2Authorization,
    pub signature: Bytes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExactEvmPayload {
    Eip3009(Eip3009Payload),
    Permit2(Permit2Payload),
}
