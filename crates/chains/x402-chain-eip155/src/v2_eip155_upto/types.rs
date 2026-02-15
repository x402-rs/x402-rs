//! Type definitions for the V2 EIP-155 "upto" payment scheme.
//!
//! This module defines types for the "upto" scheme which authorizes a transfer
//! of up to a maximum amount, where the actual amount is determined at settlement.

use alloy_primitives::U256;
use serde::{Deserialize, Serialize};
use x402_types::proto::v2;
use x402_types::{lit_str, proto};

use crate::chain::ChecksummedAddress;
use crate::chain::permit2::Permit2Payload;

lit_str!(UptoScheme, "upto");

/// Type alias for V2 verify requests using the upto EVM payment scheme.
pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;

pub type Permit2PaymentRequirements = v2::PaymentRequirements<UptoScheme, U256, ChecksummedAddress>;
pub type Permit2PaymentPayload = v2::PaymentPayload<Permit2PaymentRequirements, Permit2Payload>;

/// Settlement response for the upto scheme.
///
/// Includes the actual settled amount, which may be less than the authorized maximum.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UptoSettleResponse {
    /// Whether the settlement was successful.
    pub success: bool,
    /// Error reason if settlement failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<String>,
    /// The payer's address.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payer: Option<String>,
    /// The transaction hash (empty string for zero settlement).
    pub transaction: String,
    /// The network identifier (CAIP-2 format).
    pub network: String,
    /// The actual settled amount in atomic token units.
    pub amount: String,
}

impl UptoSettleResponse {
    /// Creates a successful settlement response.
    pub fn success(payer: String, transaction: String, network: String, amount: String) -> Self {
        Self {
            success: true,
            error_reason: None,
            payer: Some(payer),
            transaction,
            network,
            amount,
        }
    }

    /// Creates an error settlement response.
    pub fn error(network: String, reason: String) -> Self {
        Self {
            success: false,
            error_reason: Some(reason),
            payer: None,
            transaction: String::new(),
            network,
            amount: "0".to_string(),
        }
    }
}

impl From<UptoSettleResponse> for proto::SettleResponse {
    fn from(val: UptoSettleResponse) -> Self {
        proto::SettleResponse(
            serde_json::to_value(val).expect("UptoSettleResponse serialization failed"),
        )
    }
}

/// Type alias for V2 settle requests (same structure as verify requests).
pub type SettleRequest = VerifyRequest;

/// Type alias for V2 payment payloads with embedded requirements and EVM-specific data.
pub type PaymentPayload<TPaymentRequirements = PaymentRequirements> =
    v2::PaymentPayload<TPaymentRequirements, Permit2Payload>;

/// Type alias for V2 payment requirements with EVM-specific types for the upto scheme.
///
/// V2 uses CAIP-2 chain IDs and embeds requirements directly in the payload.
/// The `amount` field represents the maximum authorized amount.
pub type PaymentRequirements = v2::PaymentRequirements<UptoScheme, U256, ChecksummedAddress>;

#[cfg(any(feature = "facilitator", feature = "client"))]
pub mod facilitator_client_only {
    use alloy_sol_types::sol;

    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[derive(Debug)]
        #[sol(rpc)]
        X402UptoPermit2Proxy,
        "abi/X402UptoPermit2Proxy.json"
    );

    sol!(
        /// Signature struct to do settle through [`X402UptoPermit2Proxy`]
        #[allow(clippy::too_many_arguments)]
        #[derive(Debug)]
        struct PermitWitnessTransferFrom {
            ISignatureTransfer.TokenPermissions permitted;
            address spender;
            uint256 nonce;
            uint256 deadline;
            x402BasePermit2Proxy.Witness witness;
        }
    );
}

#[cfg(any(feature = "facilitator", feature = "client"))]
pub use facilitator_client_only::*;
