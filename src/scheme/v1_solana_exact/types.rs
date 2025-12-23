use serde::{Deserialize, Serialize};

use crate::chain::solana::Address;
use crate::proto;
use crate::proto::util::U64String;
pub use crate::scheme::v1_eip155_exact::ExactScheme;

pub type VerifyRequest = proto::v1::VerifyRequest<PaymentPayload, PaymentRequirements>;
pub type SettleRequest = VerifyRequest;
pub type PaymentPayload = proto::v1::PaymentPayload<ExactScheme, ExactSolanaPayload>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactSolanaPayload {
    pub transaction: String,
}

pub type PaymentRequirements =
    proto::v1::PaymentRequirements<ExactScheme, U64String, Address, SupportedPaymentKindExtra>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupportedPaymentKindExtra {
    pub fee_payer: Address,
}
