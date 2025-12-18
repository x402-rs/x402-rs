use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::fmt;
use std::fmt::Display;
use std::str::FromStr;

use crate::chain::ChainId;
use crate::scheme::SchemeHandlerSlug;

pub mod util;
pub mod v1;
pub mod v2;

pub type SettleRequest = VerifyRequest;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedPaymentKind {
    pub x402_version: u8,
    pub scheme: String,
    pub network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct SupportedResponse {
    pub kinds: Vec<SupportedPaymentKind>,
    #[serde(default)]
    pub extensions: Vec<String>,
    #[serde(default)]
    pub signers: HashMap<ChainId, Vec<String>>,
}

/// Represents the protocol version. Versions 1 and 2 are supported.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum X402Version {
    /// Version `1`.
    V1(v1::X402Version1),
    /// Version `2`.
    V2(v2::X402Version2),
}

impl X402Version {
    pub fn v1() -> X402Version {
        X402Version::V1(v1::X402Version1)
    }
    pub fn v2() -> X402Version {
        X402Version::V2(v2::X402Version2)
    }
}

impl From<X402Version> for u8 {
    fn from(version: X402Version) -> Self {
        match version {
            X402Version::V1(v) => v.into(),
            X402Version::V2(v) => v.into(),
        }
    }
}

impl TryFrom<u64> for X402Version {
    type Error = X402VersionError;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(X402Version::V1(v1::X402Version1)),
            2 => Ok(X402Version::V2(v2::X402Version2)),
            _ => Err(X402VersionError(value)),
        }
    }
}

impl Serialize for X402Version {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            X402Version::V1(v) => v.serialize(serializer),
            X402Version::V2(v) => v.serialize(serializer),
        }
    }
}

impl Display for X402Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            X402Version::V1(v) => Display::fmt(v, f),
            X402Version::V2(v) => Display::fmt(v, f),
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Unsupported x402 version: {0}")]
pub struct X402VersionError(pub u64);

impl TryFrom<u8> for X402Version {
    type Error = X402VersionError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            v1::X402Version1::VALUE => Ok(X402Version::v1()),
            v2::X402Version2::VALUE => Ok(X402Version::v2()),
            _ => Err(X402VersionError(value.into())),
        }
    }
}

impl<'de> Deserialize<'de> for X402Version {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let num = u8::deserialize(deserializer)?;
        X402Version::try_from(num).map_err(serde::de::Error::custom)
    }
}

/// Wrapper for a payment payload and requirements sent by the client to a facilitator
/// to be verified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyRequest(serde_json::Value);

impl VerifyRequest {
    pub fn into_json(self) -> serde_json::Value {
        self.0
    }

    pub fn scheme_handler_slug(&self) -> Option<SchemeHandlerSlug> {
        let x402_version = self.0.get("x402Version")?.as_u64()?;
        let x402_version = X402Version::try_from(x402_version).ok()?;
        match x402_version {
            X402Version::V1(_) => {
                let network_name = self.0.get("paymentPayload")?.get("network")?.as_str()?;
                let chain_id = ChainId::from_network_name(network_name).ok()?; // FIXME ERRORS
                let scheme = self.0.get("paymentPayload")?.get("scheme")?.as_str()?;
                let slug = SchemeHandlerSlug::new(chain_id, 1, scheme.into());
                Some(slug)
            }
            X402Version::V2(_) => {
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
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifyResponse(serde_json::Value);

/// Wrapper for a payment payload and requirements sent by the client to a facilitator
/// to be verified.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettleResponse(serde_json::Value);

#[derive(Debug, thiserror::Error)]
pub enum PaymentVerificationError {
    #[error("Decoding failure: {0}")]
    DecodingFailure(String),
    #[error("Payment amount is invalid with respect to the payment requirements")]
    InvalidPaymentAmount,
    #[error("Onchain balance is not enough to cover the payment amount")]
    InsufficientFunds,
    #[error("Payment authorization is not yet valid")]
    NotYetValid,
    #[error("Payment authorization is expired")]
    Expired,
    #[error("Payment chain id is invalid with respect to the payment requirements")]
    ChainIdMismatch,
    #[error("Payment receiver is invalid with respect to the payment requirements")]
    ReceiverMismatch,
    #[error("Payment asset is invalid with respect to the payment requirements")]
    AssetMismatch,
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

impl From<serde_json::Error> for PaymentVerificationError {
    fn from(value: serde_json::Error) -> Self {
        Self::DecodingFailure(value.to_string())
    }
}
