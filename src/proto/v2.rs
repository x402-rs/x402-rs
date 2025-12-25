use crate::chain::ChainId;
use crate::proto;
use crate::proto::v1;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fmt::{Display, Formatter};

/// Version 2 of the x402 protocol.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct X402Version2;

impl X402Version2 {
    pub const VALUE: u8 = 2;
}

impl From<X402Version2> for u8 {
    fn from(_: X402Version2) -> Self {
        X402Version2::VALUE
    }
}

impl Serialize for X402Version2 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(Self::VALUE)
    }
}

impl<'de> Deserialize<'de> for X402Version2 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let num = u8::deserialize(deserializer)?;
        if num == Self::VALUE {
            Ok(X402Version2)
        } else {
            Err(serde::de::Error::custom(format!(
                "expected version {}, got {}",
                Self::VALUE,
                num
            )))
        }
    }
}

impl Display for X402Version2 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", Self::VALUE)
    }
}

pub type VerifyResponse = v1::VerifyResponse;
pub type SettleResponse = v1::SettleResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceInfo {
    pub description: String,
    pub mime_type: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest<TPayload, TRequirements> {
    pub x402_version: X402Version2,
    pub payment_payload: TPayload,
    pub payment_requirements: TRequirements,
}

impl<TPayload, TRequirements> VerifyRequest<TPayload, TRequirements>
where
    Self: DeserializeOwned,
{
    pub fn from_proto(
        request: proto::VerifyRequest,
    ) -> Result<Self, proto::PaymentVerificationError> {
        let deserialized: Self = serde_json::from_value(request.into_json())?;
        Ok(deserialized)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload<TAccepted, TPayload> {
    pub accepted: TAccepted,
    pub payload: TPayload,
    pub resource: ResourceInfo,
    pub x402_version: X402Version2,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements<TScheme, TAmount, TAddress, TExtra> {
    pub scheme: TScheme,
    pub network: ChainId,
    pub amount: TAmount,
    pub pay_to: TAddress,
    pub max_timeout_seconds: u64,
    pub asset: TAddress,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<TExtra>,
}

/// Structured representation of a V2 Payment-Required header.
/// This provides proper typing for the payment required response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequired {
    pub x402_version: X402Version2,
    pub resource: ResourceInfo,
    pub accepts: Vec<serde_json::Value>,
}
