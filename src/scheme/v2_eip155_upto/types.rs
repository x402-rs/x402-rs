//! Type definitions for the V2 EIP-155 "upto" payment scheme.
//!
//! This module defines the wire format types for EIP-2612 permit-based batched payments
//! on EVM chains. Unlike the "exact" scheme which uses ERC-3009 for immediate settlement,
//! the "upto" scheme uses EIP-2612 permits to authorize a spending cap, enabling multiple
//! payments to be batched and settled together.

use alloy_primitives::{Address, Bytes, U256};
use serde::{Deserialize, Serialize};

use crate::chain::eip155::{ChecksummedAddress, TokenAmount};
use crate::lit_str;
use crate::proto::v2;

lit_str!(UptoScheme, "upto");

pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;
pub type SettleRequest = VerifyRequest;
pub type PaymentPayload = v2::PaymentPayload<PaymentRequirements, UptoEvmPayload>;
pub type PaymentRequirements =
    v2::PaymentRequirements<UptoScheme, TokenAmount, ChecksummedAddress, PaymentRequirementsExtra>;

/// Full payload required to authorize an EIP-2612 permit:
/// includes the signature and the authorization struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UptoEvmPayload {
    pub signature: Bytes,
    pub authorization: UptoEvmAuthorization,
}

/// EIP-2612 permit authorization data.
/// Defines who can spend how much tokens and when.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UptoEvmAuthorization {
    /// Token owner (the payer)
    pub from: Address,
    /// Spender (the facilitator)
    pub to: Address,
    /// Maximum spending cap
    pub value: U256,
    /// EIP-2612 nonce (from the token contract)
    pub nonce: U256,
    /// Deadline timestamp (seconds since epoch)
    pub valid_before: U256,
}

/// Extra requirements for upto payments.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirementsExtra {
    /// EIP-712 domain name (e.g., "USD Coin")
    pub name: String,
    /// EIP-712 domain version (e.g., "2")
    pub version: String,
    /// Optional maximum cap requirement
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_amount_required: Option<U256>,
}
