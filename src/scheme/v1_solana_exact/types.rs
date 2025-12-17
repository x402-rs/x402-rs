use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};

use crate::chain::solana::Address;
use crate::proto;
use crate::proto::util::U64String;
use crate::proto::v1::X402Version1;

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

/// Wrapper for a payment payload and requirements sent by the client to a facilitator
/// to be verified.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest {
    pub x402_version: X402Version1,
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
}

impl VerifyRequest {
    pub fn from_proto(request: proto::VerifyRequest) -> Option<Self> {
        serde_json::from_value(request.into_json())
            .inspect_err(|e| tracing::error!("{:?}", e))
            .ok()
    }
}

pub type SettleRequest = VerifyRequest;

/// Describes a signed request to transfer a specific amount of funds on-chain.
/// Includes the scheme, network, and signed payload contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload {
    pub x402_version: X402Version1,
    pub scheme: ExactScheme,
    pub network: String,
    pub payload: ExactPaymentPayload,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactPaymentPayload {
    pub transaction: String,
}

/// Requirements set by the payment-gated endpoint for an acceptable payment.
/// This includes min/max amounts, recipient, asset, network, and metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements {
    pub scheme: ExactScheme,
    pub network: String,
    pub max_amount_required: U64String,
    pub resource: String,
    pub description: String,
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
    pub pay_to: Address,
    pub max_timeout_seconds: u64,
    pub asset: Address,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedPaymentKindExtra {
    pub fee_payer: Address,
}
