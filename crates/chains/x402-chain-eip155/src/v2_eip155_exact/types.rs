//! Type definitions for the V2 EIP-155 "exact" payment scheme.
//!
//! This module re-exports types from V1 and defines V2-specific wire format
//! types for ERC-3009 based payments on EVM chains.

use alloy_primitives::Bytes;
use serde::{Deserialize, Serialize};
use x402_types::proto;
use x402_types::proto::v2;
use x402_types::timestamp::UnixTimestamp;

use crate::chain::{AssetTransferMethod, ChecksummedAddress, DecimalU256, TokenAmount};

/// Re-export the "exact" scheme identifier from V1 (same for both versions).
pub use crate::v1_eip155_exact::types::{ExactEvmPayload as Eip3009Payload, ExactScheme};

/// Type alias for V2 verify requests using the exact EVM payment scheme.
pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
// FIXME Feature facilitator
pub enum FacilitatorVerifyRequest {
    #[serde(rename_all = "camelCase")]
    Eip3009 {
        /// Protocol version (always 2).
        x402_version: v2::X402Version2,
        /// The signed payment authorization.
        payment_payload: Eip3009PaymentPayload,
        /// The payment requirements to verify against.
        payment_requirements: Eip3009PaymentRequirements,
    },
    #[serde(rename_all = "camelCase")]
    Permit2 {
        /// Protocol version (always 2).
        x402_version: v2::X402Version2,
        /// The signed payment authorization.
        payment_payload: Permit2PaymentPayload,
        /// The payment requirements to verify against.
        payment_requirements: Permit2PaymentRequirements,
    },
}

// FIXME Feature facilitator
pub type Eip3009PaymentRequirements = v2::PaymentRequirements<
    ExactScheme,
    TokenAmount,
    ChecksummedAddress,
    asset_transfer_method::Eip3009,
>;
// FIXME Feature facilitator
pub type Eip3009PaymentPayload = v2::PaymentPayload<Eip3009PaymentRequirements, Eip3009Payload>;

// FIXME Feature facilitator
pub type Permit2PaymentRequirements = v2::PaymentRequirements<
    ExactScheme,
    TokenAmount,
    ChecksummedAddress,
    asset_transfer_method::Permit2,
>;
// FIXME Feature facilitator
pub type Permit2PaymentPayload = v2::PaymentPayload<Permit2PaymentRequirements, Permit2Payload>;

/// Type alias for V2 settle requests (same structure as verify requests).
pub type SettleRequest = VerifyRequest;

/// Type alias for V2 payment payloads with embedded requirements and EVM-specific data.
pub type PaymentPayload<TPaymentRequirements = PaymentRequirements> =
    v2::PaymentPayload<TPaymentRequirements, ExactEvmPayload>;

/// Type alias for V2 payment requirements with EVM-specific types.
///
/// V2 uses CAIP-2 chain IDs and embeds requirements directly in the payload,
/// unlike V1 which uses network names and separate requirement objects.
pub type PaymentRequirements =
    v2::PaymentRequirements<ExactScheme, TokenAmount, ChecksummedAddress, AssetTransferMethod>;

pub mod asset_transfer_method {
    use crate::chain::AssetTransferMethod;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
    #[serde(rename_all = "lowercase")]
    pub enum Permit2Tag {
        Permit2,
    }

    #[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
    #[serde(rename_all = "camelCase")]
    pub struct Permit2 {
        asset_transfer_method: Permit2Tag,
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    pub struct Eip3009 {
        pub name: String,
        pub version: String,
    }

    impl<'de> Deserialize<'de> for Eip3009 {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            let asset_transfer_method: AssetTransferMethod =
                AssetTransferMethod::deserialize(deserializer)?;
            match asset_transfer_method {
                AssetTransferMethod::Eip3009 { name, version } => Ok(Eip3009 { name, version }),
                AssetTransferMethod::Permit2 => Err(serde::de::Error::custom(
                    "expected EIP-3009 asset transfer method, got Permit2".to_string(),
                )),
            }
        }
    }

    impl Serialize for Eip3009 {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let asset_transfer_method = AssetTransferMethod::Eip3009 {
                name: self.name.clone(),
                version: self.version.clone(),
            };
            asset_transfer_method.serialize(serializer)
        }
    }
}

impl TryFrom<proto::VerifyRequest> for FacilitatorVerifyRequest {
    type Error = proto::PaymentVerificationError;

    fn try_from(value: proto::VerifyRequest) -> Result<Self, Self::Error> {
        println!("l.0");
        let value = serde_json::from_str(value.as_str())?;
        Ok(value)
    }
}

// FIXME Docs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Authorization {
    pub deadline: UnixTimestamp,
    pub from: ChecksummedAddress,
    pub nonce: DecimalU256,
    pub permitted: Permit2AuthorizationPermitted,
    pub spender: ChecksummedAddress,
    pub witness: Permit2Witness,
}

// FIXME Docs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Witness {
    pub extra: Bytes,
    pub to: ChecksummedAddress,
    pub valid_after: UnixTimestamp,
}

// FIXME Docs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2AuthorizationPermitted {
    pub amount: TokenAmount,
    pub token: ChecksummedAddress,
}

// FIXME Docs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Payload {
    pub permit_2_authorization: Permit2Authorization,
    pub signature: Bytes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExactEvmPayload {
    Eip3009(Eip3009Payload),
    Permit2(Permit2Payload),
}
