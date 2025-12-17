use crate::chain::ChainId;
use crate::chain::solana::Address;
use crate::proto;
use crate::proto::util::U64String;
use crate::proto::v2::{ResourceInfo, X402Version2};
use crate::scheme::v1_eip155_exact::types::ExactScheme;
use crate::scheme::v1_solana_exact::types::SupportedPaymentKindExtra;
use serde::{Deserialize, Serialize};

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

pub type SettleRequest = VerifyRequest;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload {
    pub accepted: PaymentRequirements,
    pub payload: ExactSolanaPayload,
    pub resource: ResourceInfo,
    pub x402_version: X402Version2,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements {
    pub scheme: ExactScheme,
    pub network: ChainId,
    pub amount: U64String,
    pub pay_to: Address,
    pub max_timeout_seconds: u64,
    pub asset: Address,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<SupportedPaymentKindExtra>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactSolanaPayload {
    pub transaction: String,
}
