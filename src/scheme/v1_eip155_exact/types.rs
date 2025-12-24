use alloy_primitives::{Address, B256, Bytes, U256};
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

use crate::lit_str;
use crate::proto::v1;
use crate::timestamp::UnixTimestamp;

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
