//! Protocol types for x402 payment messages.
//!
//! This module defines the wire format types used in the x402 protocol for
//! communication between buyers, sellers, and facilitators. It supports both
//! protocol version 1 (V1) and version 2 (V2).
//!
//! # Protocol Versions
//!
//! - **V1** ([`v1`]): Original protocol with network names and simpler structure
//! - **V2** ([`v2`]): Enhanced protocol with CAIP-2 chain IDs and richer metadata
//!
//! # Key Types
//!
//! - [`SupportedPaymentKind`] - Describes a payment method supported by a facilitator
//! - [`SupportedResponse`] - Response from facilitator's `/supported` endpoint
//! - [`VerifyRequest`] / [`VerifyResponse`] - Payment verification messages
//! - [`SettleRequest`] / [`SettleResponse`] - Payment settlement messages
//! - [`PaymentVerificationError`] - Errors that can occur during verification
//! - [`PaymentProblem`] - Structured error response for payment failures
//!
//! # Wire Format
//!
//! All types serialize to JSON using camelCase field names. The protocol version
//! is indicated by the `x402Version` field in payment payloads.

use serde::{Deserialize, Serialize};
use serde_with::{VecSkipError, serde_as};
use std::collections::HashMap;
use std::str::FromStr;
use x402_types::chain::ChainId;
use crate::scheme::SchemeHandlerSlug;

pub mod util;
pub mod v1;
pub mod v2;

/// Trait for types that have both V1 and V2 protocol variants.
///
/// This trait enables generic handling of protocol-versioned types through
/// the [`ProtocolVersioned`] enum.
pub trait ProtocolV {
    /// The V1 protocol variant of this type.
    type V1;
    /// The V2 protocol variant of this type.
    type V2;
}

/// A versioned protocol type that can be either V1 or V2.
///
/// This enum wraps protocol-specific types to allow handling both versions
/// in a unified way.
pub enum ProtocolVersioned<T>
where
    T: ProtocolV,
{
    /// Protocol version 1 variant.
    #[allow(dead_code)]
    V1(T::V1),
    /// Protocol version 2 variant.
    #[allow(dead_code)]
    V2(T::V2),
}

/// Describes a payment method supported by a facilitator.
///
/// This type is returned in the [`SupportedResponse`] to indicate what
/// payment schemes, networks, and protocol versions a facilitator can handle.
///
/// # Example
///
/// ```json
/// {
///   "x402Version": 2,
///   "scheme": "exact",
///   "network": "eip155:8453"
/// }
/// ```
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedPaymentKind {
    /// The x402 protocol version (1 or 2).
    pub x402_version: u8,
    /// The payment scheme identifier (e.g., "exact").
    pub scheme: String,
    /// The network identifier (CAIP-2 chain ID for V2, network name for V1).
    pub network: String,
    /// Optional scheme-specific extra data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

/// Response from a facilitator's `/supported` endpoint.
///
/// This response tells clients what payment methods the facilitator supports,
/// including protocol versions, schemes, networks, and signer addresses.
///
/// # Example
///
/// ```json
/// {
///   "kinds": [
///     { "x402Version": 2, "scheme": "exact", "network": "eip155:8453" }
///   ],
///   "extensions": [],
///   "signers": {
///     "eip155:8453": ["0x1234..."]
///   }
/// }
/// ```
#[serde_as]
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct SupportedResponse {
    /// List of supported payment kinds.
    #[serde_as(as = "VecSkipError<_>")]
    pub kinds: Vec<SupportedPaymentKind>,
    /// List of supported protocol extensions.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Map of chain IDs to signer addresses for that chain.
    #[serde(default)]
    pub signers: HashMap<ChainId, Vec<String>>,
}

/// Request to verify a payment before settlement.
///
/// This wrapper contains the payment payload and requirements sent by a client
/// to a facilitator for verification. The facilitator checks that the payment
/// authorization is valid, properly signed, and matches the requirements.
///
/// The inner JSON structure varies by protocol version and scheme.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyRequest(serde_json::Value);

/// Request to settle a verified payment on-chain.
///
/// This is the same structure as [`VerifyRequest`], containing the payment
/// payload that was previously verified.
pub type SettleRequest = VerifyRequest;

impl From<serde_json::Value> for VerifyRequest {
    fn from(value: serde_json::Value) -> Self {
        Self(value)
    }
}

impl VerifyRequest {
    /// Consumes the request and returns the inner JSON value.
    pub fn into_json(self) -> serde_json::Value {
        self.0
    }

    /// Extracts the scheme handler slug from the request.
    ///
    /// This determines which scheme handler should process this payment
    /// based on the protocol version, chain ID, and scheme name.
    ///
    /// Returns `None` if the request format is invalid or the scheme is unknown.
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

/// Response from a payment verification request.
///
/// Contains the verification result as JSON. The structure varies by
/// protocol version and scheme.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResponse(pub serde_json::Value);

/// Response from a payment settlement request.
///
/// Contains the settlement result as JSON, typically including the
/// transaction hash if settlement was successful.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettleResponse(pub serde_json::Value);

/// Errors that can occur during payment verification.
///
/// These errors are returned when a payment fails validation checks
/// performed by the facilitator before settlement.
#[derive(Debug, thiserror::Error)]
pub enum PaymentVerificationError {
    /// The payment payload format is invalid or malformed.
    #[error("Invalid format: {0}")]
    InvalidFormat(String),
    /// The payment amount doesn't match the requirements.
    #[error("Payment amount is invalid with respect to the payment requirements")]
    InvalidPaymentAmount,
    /// The payment authorization's `validAfter` timestamp is in the future.
    #[error("Payment authorization is not yet valid")]
    Early,
    /// The payment authorization's `validBefore` timestamp has passed.
    #[error("Payment authorization is expired")]
    Expired,
    /// The payment's chain ID doesn't match the requirements.
    #[error("Payment chain id is invalid with respect to the payment requirements")]
    ChainIdMismatch,
    /// The payment recipient doesn't match the requirements.
    #[error("Payment recipient is invalid with respect to the payment requirements")]
    RecipientMismatch,
    /// The payment asset (token) doesn't match the requirements.
    #[error("Payment asset is invalid with respect to the payment requirements")]
    AssetMismatch,
    /// The payer's on-chain balance is insufficient.
    #[error("Onchain balance is not enough to cover the payment amount")]
    InsufficientFunds,
    /// The payment signature is invalid.
    #[error("{0}")]
    InvalidSignature(String),
    /// Transaction simulation failed.
    #[error("{0}")]
    TransactionSimulation(String),
    /// The chain is not supported by this facilitator.
    #[error("Unsupported chain")]
    UnsupportedChain,
    /// The payment scheme is not supported by this facilitator.
    #[error("Unsupported scheme")]
    UnsupportedScheme,
    /// The accepted payment details don't match the requirements.
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

/// Machine-readable error reason codes for payment failures.
///
/// These codes are used in error responses to allow clients to
/// programmatically handle different failure scenarios.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorReason {
    /// The payment payload format is invalid.
    InvalidFormat,
    /// The payment amount is incorrect.
    InvalidPaymentAmount,
    /// The payment authorization is not yet valid.
    InvalidPaymentEarly,
    /// The payment authorization has expired.
    InvalidPaymentExpired,
    /// The chain ID doesn't match.
    ChainIdMismatch,
    /// The recipient address doesn't match.
    RecipientMismatch,
    /// The token asset doesn't match.
    AssetMismatch,
    /// The accepted details don't match requirements.
    AcceptedRequirementsMismatch,
    /// The signature is invalid.
    InvalidSignature,
    /// Transaction simulation failed.
    TransactionSimulation,
    /// Insufficient on-chain balance.
    InsufficientFunds,
    /// The chain is not supported.
    UnsupportedChain,
    /// The scheme is not supported.
    UnsupportedScheme,
    /// An unexpected error occurred.
    UnexpectedError,
}

/// Trait for converting errors into structured payment problems.
pub trait AsPaymentProblem {
    /// Converts this error into a [`PaymentProblem`].
    fn as_payment_problem(&self) -> PaymentProblem;
}

/// A structured payment error with reason code and details.
///
/// This type is used to return detailed error information to clients
/// when a payment fails verification or settlement.
pub struct PaymentProblem {
    /// The machine-readable error reason.
    reason: ErrorReason,
    /// Human-readable error details.
    details: String,
}

impl PaymentProblem {
    /// Creates a new payment problem with the given reason and details.
    pub fn new(reason: ErrorReason, details: String) -> Self {
        Self { reason, details }
    }

    /// Returns the error reason code.
    pub fn reason(&self) -> ErrorReason {
        self.reason
    }

    /// Returns the human-readable error details.
    pub fn details(&self) -> &str {
        &self.details
    }
}

/// Protocol version marker for [`PaymentRequired`] responses.
pub struct PaymentRequiredV;

impl ProtocolV for PaymentRequiredV {
    type V1 = v1::PaymentRequired;
    type V2 = v2::PaymentRequired;
}

/// A payment required response that can be either V1 or V2.
///
/// This is returned with HTTP 402 status to indicate that payment is required.
pub type PaymentRequired = ProtocolVersioned<PaymentRequiredV>;
