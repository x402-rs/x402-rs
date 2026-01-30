//! Type definitions for the V1 EIP-155 "exact" payment scheme.
//!
//! This module defines the wire format types for ERC-3009 based payments
//! on EVM chains using the V1 x402 protocol.

use alloy_primitives::{Address, B256, Bytes, U256};
use serde::{Deserialize, Serialize};
use x402_types::lit_str;
use x402_types::proto::v1;
use x402_types::timestamp::UnixTimestamp;

#[cfg(any(feature = "facilitator", feature = "client"))]
use alloy_sol_types::sol;

lit_str!(ExactScheme, "exact");

pub type VerifyRequest = v1::VerifyRequest<PaymentPayload, PaymentRequirements>;
pub type SettleRequest = VerifyRequest;
pub type PaymentPayload = v1::PaymentPayload<ExactScheme, ExactEvmPayload>;

/// Full payload required to authorize an ERC-3009 transfer:
/// includes the signature and the EIP-712 struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactEvmPayload {
    pub signature: Bytes,
    pub authorization: ExactEvmPayloadAuthorization,
}

/// EIP-712 structured data for ERC-3009-based authorization.
/// Defines who can transfer how much tokens and when.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactEvmPayloadAuthorization {
    pub from: Address,
    pub to: Address,
    pub value: U256,
    pub valid_after: UnixTimestamp,
    pub valid_before: UnixTimestamp,
    pub nonce: B256,
}

pub type PaymentRequirements =
    v1::PaymentRequirements<ExactScheme, U256, Address, PaymentRequirementsExtra>;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirementsExtra {
    pub name: String,
    pub version: String,
}

#[cfg(any(feature = "facilitator", feature = "client"))]
sol!(
    /// Solidity-compatible struct definition for ERC-3009 `transferWithAuthorization`.
    ///
    /// This matches the EIP-3009 format used in EIP-712 typed data:
    /// it defines the authorization to transfer tokens from `from` to `to`
    /// for a specific `value`, valid only between `validAfter` and `validBefore`
    /// and identified by a unique `nonce`.
    ///
    /// This struct is primarily used to reconstruct the typed data domain/message
    /// when verifying a client's signature.
    #[derive(Serialize, Deserialize)]
    struct TransferWithAuthorization {
        address from;
        address to;
        uint256 value;
        uint256 validAfter;
        uint256 validBefore;
        bytes32 nonce;
    }
);
