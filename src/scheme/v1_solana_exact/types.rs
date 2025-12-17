use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

use crate::chain::solana::Address;
use crate::proto;
use crate::proto::util::U64String;

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

pub type VerifyRequest = proto::v1::VerifyRequest<PaymentPayload, PaymentRequirements>;
pub type SettleRequest = VerifyRequest;
pub type PaymentPayload = proto::v1::PaymentPayload<ExactScheme, ExactPaymentPayload>;

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
    pub extra: Option<SupportedPaymentKindExtra>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupportedPaymentKindExtra {
    pub fee_payer: Address,
}
