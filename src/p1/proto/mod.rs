use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashMap;
use std::fmt;
use std::fmt::Display;
use std::str::FromStr;
use bincode::Options;

use crate::p1::chain::ChainId;
use crate::p1::scheme::SchemeHandlerSlug;

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

impl Into<u8> for X402Version {
    fn into(self) -> u8 {
        match self {
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
                let chain_id = ChainId::from_network_name(network_name)?;
                let scheme = self.0.get("paymentPayload")?.get("scheme")?.as_str()?;
                let slug = SchemeHandlerSlug::new(chain_id, 1, scheme.into());
                Some(slug)
            }
            X402Version::V2(_) => None,
        }
    }
}

/// Result returned by a facilitator after verifying a [`PaymentPayload`] against the provided [`PaymentRequirements`].
///
/// This response indicates whether the payment authorization is valid and identifies the payer. If invalid,
/// it includes a reason describing why verification failed (e.g., wrong network, an invalid scheme, insufficient funds).
#[derive(Debug)]
pub enum VerifyResponse {
    /// The payload matches the requirements and passes all checks.
    Valid { payer: String },
    /// The payload was well-formed but failed verification due to the specified [`FacilitatorErrorReason`]
    Invalid {
        reason: String,
        payer: Option<String>,
    },
}

impl VerifyResponse {
    /// Constructs a successful verification response with the given `payer` address.
    ///
    /// Indicates that the provided payment payload has been validated against the payment requirements.
    pub fn valid(payer: String) -> Self {
        VerifyResponse::Valid { payer }
    }

    /// Constructs a failed verification response with the given `payer` address and error `reason`.
    ///
    /// Indicates that the payment was recognized but rejected due to reasons such as
    /// insufficient funds, invalid network, or scheme mismatch.
    pub fn invalid(payer: Option<String>, reason: String) -> Self {
        VerifyResponse::Invalid { reason, payer }
    }
}

impl Serialize for VerifyResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut s = match self {
            VerifyResponse::Valid { .. } => serializer.serialize_struct("VerifyResponse", 2)?,
            VerifyResponse::Invalid { .. } => serializer.serialize_struct("VerifyResponse", 3)?,
        };

        match self {
            VerifyResponse::Valid { payer } => {
                s.serialize_field("isValid", &true)?;
                s.serialize_field("payer", payer)?;
            }
            VerifyResponse::Invalid { reason, payer } => {
                s.serialize_field("isValid", &false)?;
                s.serialize_field("invalidReason", reason)?;
                if let Some(payer) = payer {
                    s.serialize_field("payer", payer)?
                }
            }
        }

        s.end()
    }
}

impl<'de> Deserialize<'de> for VerifyResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            is_valid: bool,
            #[serde(skip_serializing_if = "Option::is_none")]
            payer: Option<String>,
            #[serde(default)]
            invalid_reason: Option<String>,
        }

        let raw = Raw::deserialize(deserializer)?;

        match (raw.is_valid, raw.invalid_reason) {
            (true, None) => match raw.payer {
                None => Err(serde::de::Error::custom(
                    "`payer` must be present when `isValid` is true",
                )),
                Some(payer) => Ok(VerifyResponse::Valid { payer }),
            },
            (false, Some(reason)) => Ok(VerifyResponse::Invalid {
                payer: raw.payer,
                reason,
            }),
            (true, Some(_)) => Err(serde::de::Error::custom(
                "`invalidReason` must be absent when `isValid` is true",
            )),
            (false, None) => Err(serde::de::Error::custom(
                "`invalidReason` must be present when `isValid` is false",
            )),
        }
    }
}

/// Returned from a facilitator after attempting to settle a payment on-chain.
/// Indicates success/failure, transaction hash, and payer identity.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<String>,
    pub payer: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<String>,
    pub network: String,
}
