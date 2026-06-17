//! Wire format types for the V2 TRON "exact" payment scheme.

use serde::{Deserialize, Serialize};

lit_str!(ExactScheme, "exact");

// ──────────────────────────────────────────────
// Asset transfer method extra
// ──────────────────────────────────────────────

/// Extra data in PaymentRequirements describing how the asset is transferred.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "assetTransferMethod", rename_all = "camelCase")]
pub enum TronAssetTransferMethod {
    /// EIP-3009 `transferWithAuthorization`. Requires EIP-712 domain name/version.
    #[serde(rename = "eip3009")]
    Eip3009 {
        /// Token name for EIP-712 domain.
        name: String,
        /// Token version for EIP-712 domain.
        version: String,
    },
    /// Permit2-based transfer.
    #[serde(rename = "permit2")]
    Permit2,
}

// ──────────────────────────────────────────────
// Payment requirements
// ──────────────────────────────────────────────

#[cfg(feature = "facilitator")]
pub use facilitator_types::*;
use x402_types::lit_str;

#[cfg(feature = "facilitator")]
mod facilitator_types {
    use alloy_primitives::{Address, B256, Bytes, U256};
    use serde::{Deserialize, Serialize};
    use x402_types::proto::{self, v2};
    use x402_types::timestamp::UnixTimestamp;
    use x402_types::util::DecimalU256;

    use crate::chain::TronAddress;
    use crate::v2_tron_exact::{ExactScheme, TronAssetTransferMethod};

    /// Type alias for TRON V2 payment requirements.
    pub type PaymentRequirements =
        v2::PaymentRequirements<ExactScheme, DecimalU256, TronAddress, TronAssetTransferMethod>;

    /// EIP-3009-style payment requirements (extra carries name/version).
    pub type Eip3009PaymentRequirements =
        v2::PaymentRequirements<ExactScheme, DecimalU256, TronAddress, Eip3009Extra>;

    /// Permit2 payment requirements.
    pub type Permit2PaymentRequirements =
        v2::PaymentRequirements<ExactScheme, DecimalU256, TronAddress, Permit2Extra>;

    /// Extra for EIP-3009: name + version for TIP-712 domain.
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Eip3009Extra {
        #[serde(rename = "assetTransferMethod")]
        pub asset_transfer_method: Eip3009Tag,
        pub name: String,
        pub version: String,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Eip3009Tag {
        Eip3009,
    }

    /// Extra for Permit2 (no additional fields beyond the tag).
    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Permit2Extra {
        #[serde(rename = "assetTransferMethod")]
        pub asset_transfer_method: Permit2Tag,
    }

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(rename_all = "lowercase")]
    pub enum Permit2Tag {
        Permit2,
    }

    // ──────────────────────────────────────────────
    // Payload types
    // ──────────────────────────────────────────────

    /// EIP-3009 authorization fields. Addresses are EVM hex (0x...).
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Eip3009Authorization {
        pub from: Address,
        pub to: Address,
        pub value: DecimalU256,
        pub valid_after: UnixTimestamp,
        pub valid_before: UnixTimestamp,
        pub nonce: B256,
    }

    /// Full EIP-3009 payload: authorization + signature.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Eip3009Payload {
        pub authorization: Eip3009Authorization,
        pub signature: Bytes,
    }

    /// Witness for Permit2 (matches the sol! struct).
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Permit2Witness {
        pub to: Address,
        pub valid_after: UnixTimestamp,
    }

    /// TokenPermissions for Permit2.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Permit2TokenPermissions {
        pub token: Address,
        pub amount: DecimalU256,
    }

    /// Permit2 authorization struct (matches the Permit2 contract's PermitTransferFrom).
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Permit2Authorization {
        /// The payer (owner of the tokens).
        pub from: Address,
        pub permitted: Permit2TokenPermissions,
        pub spender: Address,
        /// Permit2 bitmap nonce — sent as a decimal string on the wire.
        pub nonce: DecimalU256,
        pub deadline: UnixTimestamp,
        pub witness: Permit2Witness,
    }

    /// Full Permit2 payload: authorization + signature.
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(rename_all = "camelCase")]
    pub struct Permit2Payload {
        pub permit2_authorization: Permit2Authorization,
        pub signature: Bytes,
    }

    pub type Eip3009PaymentPayload = v2::PaymentPayload<Eip3009PaymentRequirements, Eip3009Payload>;
    pub type Permit2PaymentPayload = v2::PaymentPayload<Permit2PaymentRequirements, Permit2Payload>;

    /// The typed verify/settle request (discriminated by extra field).
    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(untagged)]
    pub enum FacilitatorVerifyRequest {
        #[serde(rename_all = "camelCase")]
        Eip3009 {
            x402_version: v2::X402Version2,
            payment_payload: Eip3009PaymentPayload,
            payment_requirements: Eip3009PaymentRequirements,
        },
        #[serde(rename_all = "camelCase")]
        Permit2 {
            x402_version: v2::X402Version2,
            payment_payload: Permit2PaymentPayload,
            payment_requirements: Permit2PaymentRequirements,
        },
    }

    impl TryFrom<proto::VerifyRequest> for FacilitatorVerifyRequest {
        type Error = proto::PaymentVerificationError;

        fn try_from(value: proto::VerifyRequest) -> Result<Self, Self::Error> {
            let v = serde_json::from_str(value.as_str())?;
            Ok(v)
        }
    }

    pub type FacilitatorSettleRequest = FacilitatorVerifyRequest;
}
