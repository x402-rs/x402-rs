use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::str::FromStr;

use crate::chain::ChainId;
use crate::scheme::SchemeHandlerSlug;

pub mod util;
pub mod v1;
pub mod v2;

pub trait ProtocolV {
    type V1;
    type V2;
}

pub enum ProtocolVersioned<T>
where
    T: ProtocolV,
{
    #[allow(dead_code)]
    V1(T::V1),
    #[allow(dead_code)]
    V2(T::V2),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedPaymentKind {
    pub x402_version: u8,
    pub scheme: String,
    pub network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct SupportedResponse {
    pub kinds: Vec<SupportedPaymentKind>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub signers: HashMap<ChainId, Vec<String>>,
}

/// Wrapper for a payment payload and requirements sent by the client to a facilitator
/// to be verified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyRequest(serde_json::Value);

pub type SettleRequest = VerifyRequest;

impl From<serde_json::Value> for VerifyRequest {
    fn from(value: serde_json::Value) -> Self {
        Self(value)
    }
}

impl VerifyRequest {
    pub fn into_json(self) -> serde_json::Value {
        self.0
    }

    pub fn scheme_handler_slug(&self) -> Option<SchemeHandlerSlug> {
        let x402_version: u8 = self.0.get("x402Version")?.as_u64()?.try_into().ok()?;
        match x402_version {
            v1::X402Version1::VALUE => {
                let network_name = self.0.get("paymentPayload")?.get("network")?.as_str()?;
                let chain_id = ChainId::from_network_name(network_name)?;
                let scheme = self.0.get("paymentPayload")?.get("scheme")?.as_str()?;
                let slug = SchemeHandlerSlug::new(chain_id, 1, scheme.into());
                Some(slug)
            }
            v2::X402Version2::VALUE => {
                let chain_id_string = self
                    .0
                    .get("paymentPayload")?
                    .get("accepted")?
                    .get("network")?
                    .as_str()?;
                let chain_id = ChainId::from_str(chain_id_string).ok()?;
                let scheme = self
                    .0
                    .get("paymentPayload")?
                    .get("accepted")?
                    .get("scheme")?
                    .as_str()?;
                let slug = SchemeHandlerSlug::new(chain_id, 2, scheme.into());
                Some(slug)
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResponse(pub serde_json::Value);

/// Wrapper for a payment payload and requirements sent by the client to a facilitator
/// to be verified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettleResponse(pub serde_json::Value);

#[derive(Debug, thiserror::Error)]
pub enum PaymentVerificationError {
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
    #[error("Payment amount is invalid with respect to the payment requirements")]
    InvalidPaymentAmount,
    #[error("Payment authorization is not yet valid")]
    Early,
    #[error("Payment authorization is expired")]
    Expired,
    #[error("Payment chain id is invalid with respect to the payment requirements")]
    ChainIdMismatch,
    #[error("Payment recipient is invalid with respect to the payment requirements")]
    RecipientMismatch,
    #[error("Payment asset is invalid with respect to the payment requirements")]
    AssetMismatch,
    #[error("Onchain balance is not enough to cover the payment amount")]
    InsufficientFunds,
    #[error("{0}")]
    InvalidSignature(String),
    #[error("{0}")]
    TransactionSimulation(String),
    #[error("Unsupported chain")]
    UnsupportedChain,
    #[error("Unsupported scheme")]
    UnsupportedScheme,
    #[error("Accepted does not match payment requirements")]
    AcceptedRequirementsMismatch,
}

impl AsPaymentProblem for PaymentVerificationError {
    fn as_payment_problem(&self) -> PaymentProblem {
        let error_reason = match self {
            PaymentVerificationError::InvalidFormat(_) => ErrorReason::InvalidFormat,
            PaymentVerificationError::InvalidPaymentAmount => ErrorReason::InvalidPaymentAmount,
            PaymentVerificationError::InsufficientFunds => ErrorReason::InsufficientFunds,
            PaymentVerificationError::Early => ErrorReason::InvalidPaymentEarly,
            PaymentVerificationError::Expired => ErrorReason::InvalidPaymentExpired,
            PaymentVerificationError::ChainIdMismatch => ErrorReason::ChainIdMismatch,
            PaymentVerificationError::RecipientMismatch => ErrorReason::RecipientMismatch,
            PaymentVerificationError::AssetMismatch => ErrorReason::AssetMismatch,
            PaymentVerificationError::InvalidSignature(_) => ErrorReason::InvalidSignature,
            PaymentVerificationError::TransactionSimulation(_) => {
                ErrorReason::TransactionSimulation
            }
            PaymentVerificationError::UnsupportedChain => ErrorReason::UnsupportedChain,
            PaymentVerificationError::UnsupportedScheme => ErrorReason::UnsupportedScheme,
            PaymentVerificationError::AcceptedRequirementsMismatch => {
                ErrorReason::AcceptedRequirementsMismatch
            }
        };
        PaymentProblem::new(error_reason, self.to_string())
    }
}

impl From<serde_json::Error> for PaymentVerificationError {
    fn from(value: serde_json::Error) -> Self {
        Self::InvalidFormat(value.to_string())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorReason {
    InvalidFormat,
    InvalidPaymentAmount,
    InvalidPaymentEarly,
    InvalidPaymentExpired,
    ChainIdMismatch,
    RecipientMismatch,
    AssetMismatch,
    AcceptedRequirementsMismatch,
    InvalidSignature,
    TransactionSimulation,
    InsufficientFunds,
    UnsupportedChain,
    UnsupportedScheme,
    UnexpectedError,
}

pub trait AsPaymentProblem {
    fn as_payment_problem(&self) -> PaymentProblem;
}

pub struct PaymentProblem {
    reason: ErrorReason,
    details: String,
}

impl PaymentProblem {
    pub fn new(reason: ErrorReason, details: String) -> Self {
        Self { reason, details }
    }
    pub fn reason(&self) -> ErrorReason {
        self.reason
    }
    pub fn details(&self) -> &str {
        &self.details
    }
}

pub struct PaymentRequiredV;

impl ProtocolV for PaymentRequiredV {
    type V1 = v1::PaymentRequired;
    type V2 = v2::PaymentRequired;
}

pub type PaymentRequired = ProtocolVersioned<PaymentRequiredV>;
