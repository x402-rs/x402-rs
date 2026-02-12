//! Type definitions for the V2 EIP-155 "upto" payment scheme.
//!
//! This module defines types for the "upto" scheme which authorizes a transfer
//! of up to a maximum amount, where the actual amount is determined at settlement.

use alloy_primitives::{Bytes, U256};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use x402_types::proto::v2;
use x402_types::timestamp::UnixTimestamp;

use crate::chain::ChecksummedAddress;

/// The "upto" scheme identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "&str", into = "&str")]
pub struct UptoScheme;

impl UptoScheme {
    /// Returns the scheme name as a string.
    pub const fn as_str(&self) -> &'static str {
        "upto"
    }
}

impl std::fmt::Display for UptoScheme {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl AsRef<str> for UptoScheme {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl From<UptoScheme> for &'static str {
    fn from(val: UptoScheme) -> Self {
        val.as_str()
    }
}

impl TryFrom<&str> for UptoScheme {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if value == "upto" {
            Ok(UptoScheme)
        } else {
            Err(format!("Expected 'upto', got '{}'", value))
        }
    }
}

impl FromStr for UptoScheme {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::try_from(s)
    }
}

/// Type alias for V2 verify requests using the upto EVM payment scheme.
pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;

#[cfg(feature = "facilitator")]
mod facilitator_only {
    use alloy_primitives::U256;
    use serde::{Deserialize, Serialize};
    use x402_types::proto;
    use x402_types::proto::v2;

    use crate::chain::ChecksummedAddress;
    use crate::v2_eip155_upto::{Permit2Payload, UptoExtra, UptoScheme};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct FacilitatorVerifyRequest {
        /// Protocol version (always 2).
        pub x402_version: v2::X402Version2,
        /// The signed payment authorization.
        pub payment_payload: Permit2PaymentPayload,
        /// The payment requirements to verify against.
        pub payment_requirements: Permit2PaymentRequirements,
    }

    impl TryFrom<proto::VerifyRequest> for FacilitatorVerifyRequest {
        type Error = proto::PaymentVerificationError;

        fn try_from(value: proto::VerifyRequest) -> Result<Self, Self::Error> {
            let value = serde_json::from_str(value.as_str())?;
            Ok(value)
        }
    }

    pub type FacilitatorSettleRequest = FacilitatorVerifyRequest;

    pub type Permit2PaymentRequirements =
        v2::PaymentRequirements<UptoScheme, U256, ChecksummedAddress, UptoExtra>;
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
        pub fn success(
            payer: String,
            transaction: String,
            network: String,
            amount: String,
        ) -> Self {
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
}

#[cfg(feature = "facilitator")]
pub use facilitator_only::*;

/// Type alias for V2 settle requests (same structure as verify requests).
pub type SettleRequest = VerifyRequest;

/// Type alias for V2 payment payloads with embedded requirements and EVM-specific data.
pub type PaymentPayload<TPaymentRequirements = PaymentRequirements> =
    v2::PaymentPayload<TPaymentRequirements, Permit2Payload>;

/// Type alias for V2 payment requirements with EVM-specific types for the upto scheme.
///
/// V2 uses CAIP-2 chain IDs and embeds requirements directly in the payload.
/// The `amount` field represents the maximum authorized amount.
pub type PaymentRequirements =
    v2::PaymentRequirements<UptoScheme, U256, ChecksummedAddress, UptoExtra>;

/// Extra fields for upto payment requirements.
///
/// Contains token name and version for EIP-712 domain construction.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UptoExtra {
    /// The token name (e.g., "USDC")
    pub name: String,
    /// The token version (e.g., "2")
    pub version: String,
}

/// Payload for Permit2-based upto payments.
///
/// Contains the authorization details and signature for a Permit2 transfer
/// where the actual settled amount may be less than the authorized maximum.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Payload {
    pub permit_2_authorization: Permit2Authorization,
    pub signature: Bytes,
}

#[cfg(any(feature = "facilitator", feature = "client"))]
pub mod facilitator_client_only {
    use alloy_primitives::{Address, address};
    use alloy_sol_types::sol;

    /// The canonical Permit2 contract address deployed on most chains.
    pub const PERMIT2_ADDRESS: Address = address!("0x000000000022D473030F116dDEE9F6B43aC78BA3");

    /// The X402 UptoPermit2Proxy contract address for settling Permit2 payments with variable amounts.
    /// This contract allows settling for any amount up to the permitted maximum.
    pub const UPTO_PERMIT2_PROXY_ADDRESS: Address =
        address!("0x4020633461b2895a48930ff97ee8fcde8e520002");

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

/// Authorization details for a Permit2 upto payment.
///
/// The `permitted.amount` represents the maximum amount that can be charged.
/// The actual settled amount will be determined by the server at settlement time.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Authorization {
    /// Deadline after which the authorization expires.
    pub deadline: UnixTimestamp,
    /// The address authorizing the transfer (the payer).
    pub from: ChecksummedAddress,
    /// Unique nonce for replay protection.
    #[serde(with = "crate::decimal_u256")]
    pub nonce: U256,
    /// The token and maximum amount permitted.
    pub permitted: Permit2AuthorizationPermitted,
    /// The spender address (must be the X402 Permit2Proxy).
    pub spender: ChecksummedAddress,
    /// Witness data binding the recipient.
    pub witness: Permit2Witness,
}

/// Witness data for Permit2 upto payments.
///
/// Binds the recipient address to prevent the facilitator from redirecting funds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Witness {
    /// Extra data (can be empty for basic transfers).
    pub extra: Bytes,
    /// The recipient address that will receive the funds.
    pub to: ChecksummedAddress,
    /// Time after which the authorization becomes valid.
    pub valid_after: UnixTimestamp,
}

/// Token and amount details for Permit2 authorization.
///
/// The `amount` is the maximum that can be charged at settlement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2AuthorizationPermitted {
    /// Maximum amount that can be transferred.
    #[serde(with = "crate::decimal_u256")]
    pub amount: U256,
    /// Token contract address.
    pub token: ChecksummedAddress,
}
