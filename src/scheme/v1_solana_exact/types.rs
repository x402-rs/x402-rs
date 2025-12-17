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

pub type PaymentRequirements =
    proto::v1::PaymentRequirements<ExactScheme, U64String, Address, SupportedPaymentKindExtra>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupportedPaymentKindExtra {
    pub fee_payer: Address,
}
