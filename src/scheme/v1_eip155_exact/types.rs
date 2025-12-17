use alloy_primitives::{Address, B256, Bytes, U256};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

use crate::proto::v1;
use crate::timestamp::UnixTimestamp;

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum ExactScheme {
    #[serde(rename = "exact")]
    Exact, // serializes as "exact", deserializes only from "exact"
}

impl Display for ExactScheme {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "exact")
    }
}

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
/// Defines who can transfer how much USDC and when.
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

/// Requirements set by the payment-gated endpoint for an acceptable payment.
/// This includes min/max amounts, recipient, asset, network, and metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements {
    pub scheme: ExactScheme,
    pub network: String,
    pub max_amount_required: U256,
    pub resource: String,
    pub description: String,
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
    pub pay_to: Address,
    pub max_timeout_seconds: u64,
    pub asset: Address,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<PaymentRequirementsExtra>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirementsExtra {
    pub name: String,
    pub version: String,
}
