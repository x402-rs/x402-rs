use alloy_primitives::{Address, U256};
use serde::{Deserialize, Serialize};

use crate::chain::ChainId;
use crate::proto;
use crate::proto::v2::X402Version2;
use crate::scheme::v1_eip155_exact::types::{ExactEvmPayload, PaymentRequirementsExtra};

pub use crate::scheme::v1_eip155_exact::types::ExactScheme;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest {
    pub x402_version: X402Version2,
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
}

impl VerifyRequest {
    pub fn from_proto(request: proto::VerifyRequest) -> Option<Self> {
        serde_json::from_value(request.into_json()).ok()
    }
}

// TODO Unify payment payload and shared struct through generics maybe

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload {
    pub accepted: PaymentRequirements,
    pub payload: ExactEvmPayload,
    pub resource: ResourceInfo,
    pub x402_version: X402Version2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceInfo {
    pub description: String,
    pub mime_type: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements {
    pub scheme: ExactScheme,
    pub network: ChainId,
    pub amount: U256,
    pub pay_to: Address,
    pub max_timeout_seconds: u64,
    pub asset: Address,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<PaymentRequirementsExtra>,
}
