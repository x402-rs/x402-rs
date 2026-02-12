//! Type definitions for the V2 EIP-155 "exact" payment scheme.
//!
//! This module re-exports types from V1 and defines V2-specific wire format
//! types for ERC-3009 based payments on EVM chains.

use alloy_primitives::{Bytes, U256};
use serde::{Deserialize, Serialize};
use x402_types::proto::v2;
use x402_types::timestamp::UnixTimestamp;

use crate::chain::{AssetTransferMethod, ChecksummedAddress};

/// Re-export the "exact" scheme identifier from V1 (same for both versions).
pub use crate::v1_eip155_exact::types::{ExactEvmPayload as Eip3009Payload, ExactScheme};

/// Type alias for V2 verify requests using the exact EVM payment scheme.
pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;

#[cfg(feature = "facilitator")]
mod facilitator_only {
    use alloy_primitives::U256;
    use serde::{Deserialize, Serialize};
    use x402_types::proto;
    use x402_types::proto::v2;

    use crate::chain::ChecksummedAddress;
    use crate::v1_eip155_exact::ExactScheme;
    use crate::v2_eip155_exact::{Eip3009Payload, Permit2Payload, asset_transfer_method};

    #[derive(Debug, Clone, Serialize, Deserialize)]
    #[serde(untagged)]
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

    impl TryFrom<proto::VerifyRequest> for FacilitatorVerifyRequest {
        type Error = proto::PaymentVerificationError;

        fn try_from(value: proto::VerifyRequest) -> Result<Self, Self::Error> {
            let value = serde_json::from_str(value.as_str())?;
            Ok(value)
        }
    }

    pub type FacilitatorSettleRequest = FacilitatorVerifyRequest;

    pub type Eip3009PaymentRequirements = v2::PaymentRequirements<
        ExactScheme,
        U256,
        ChecksummedAddress,
        asset_transfer_method::Eip3009,
    >;
    pub type Eip3009PaymentPayload = v2::PaymentPayload<Eip3009PaymentRequirements, Eip3009Payload>;

    pub type Permit2PaymentRequirements = v2::PaymentRequirements<
        ExactScheme,
        U256,
        ChecksummedAddress,
        asset_transfer_method::Permit2,
    >;
    pub type Permit2PaymentPayload = v2::PaymentPayload<Permit2PaymentRequirements, Permit2Payload>;
}

#[cfg(feature = "facilitator")]
pub use facilitator_only::*;

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
    v2::PaymentRequirements<ExactScheme, U256, ChecksummedAddress, AssetTransferMethod>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExactEvmPayload {
    Eip3009(Eip3009Payload),
    Permit2(Permit2Payload),
}

// FIXME Docs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Payload {
    pub permit_2_authorization: Permit2Authorization,
    pub signature: Bytes,
}

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

#[cfg(any(feature = "facilitator", feature = "client"))]
pub mod facilitator_client_only {
    use alloy_primitives::{Address, address};
    use alloy_sol_types::sol;

    /// The canonical Permit2 contract address deployed on most chains.
    pub const PERMIT2_ADDRESS: Address = address!("0x000000000022D473030F116dDEE9F6B43aC78BA3");

    /// The X402 ExactPermit2Proxy contract address for settling Permit2 payments.
    pub const EXACT_PERMIT2_PROXY_ADDRESS: Address =
        address!("0x4020615294c913F045dc10f0a5cdEbd86c280001");

    sol!(
        #[allow(missing_docs)]
        #[allow(clippy::too_many_arguments)]
        #[derive(Debug)]
        #[sol(rpc)]
        X402ExactPermit2Proxy,
        "abi/X402ExactPermit2Proxy.json"
    );

    sol!(
        /// Signature struct to do settle through [`X402ExactPermit2Proxy`]
        /// Depends on availability of [`X402ExactPermit2Proxy`]
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

// FIXME Docs
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Authorization {
    pub deadline: UnixTimestamp,
    pub from: ChecksummedAddress,
    #[serde(with = "crate::decimal_u256")]
    pub nonce: U256,
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
    #[serde(with = "crate::decimal_u256")]
    pub amount: U256,
    pub token: ChecksummedAddress,
}
