//! V2 Aptos "exact" payment scheme types.

use serde::{Deserialize, Serialize};
use x402_types::lit_str;
use x402_types::proto::v2;

use crate::chain::Address;

lit_str!(ExactScheme, "exact");

/// The V2 Aptos exact scheme verify request.
pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;

/// The V2 Aptos exact scheme settle request.
pub type SettleRequest = VerifyRequest;

/// The payment payload for Aptos exact scheme.
pub type PaymentPayload = v2::PaymentPayload<PaymentRequirements, ExactAptosPayload>;

/// The payment requirements for Aptos exact scheme.
pub type PaymentRequirements =
    v2::PaymentRequirements<ExactScheme, String, Address, Option<AptosPaymentRequirementsExtra>>;

/// The transaction payload containing the base64-encoded BCS transaction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactAptosPayload {
    /// Base64-encoded JSON containing the BCS transaction and authenticator.
    pub transaction: String,
}

/// Extra requirements for sponsored transactions.
///
/// When present, `fee_payer` indicates the facilitator address that will
/// sponsor gas fees for the transaction.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AptosPaymentRequirementsExtra {
    /// The address of the fee payer (facilitator). When present, sponsorship is enabled.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fee_payer: Option<Address>,
}
