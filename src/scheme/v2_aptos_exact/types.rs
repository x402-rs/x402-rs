//! V2 Aptos "exact" payment scheme types.

use crate::chain::aptos::Address;
use crate::proto::v2;
use crate::scheme::v1_eip155_exact::types::ExactScheme;
use serde::{Deserialize, Serialize};

/// The V2 Aptos exact scheme verify request.
pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;

/// The V2 Aptos exact scheme settle request.
pub type SettleRequest = VerifyRequest;

/// The payment payload for Aptos exact scheme.
pub type PaymentPayload = v2::PaymentPayload<PaymentRequirements, ExactAptosPayload>;

/// The payment requirements for Aptos exact scheme.
pub type PaymentRequirements =
    v2::PaymentRequirements<ExactScheme, String, Address, SupportedPaymentKindExtra>;

/// The transaction payload containing the base64-encoded BCS transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactAptosPayload {
    /// Base64-encoded JSON containing the BCS transaction and authenticator.
    pub transaction: String,
}

/// Extra requirements for sponsored transactions.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupportedPaymentKindExtra {
    /// The address of the fee payer (facilitator).
    pub fee_payer: Address,
    /// Whether this is a sponsored (gasless) transaction.
    pub sponsored: bool,
}
