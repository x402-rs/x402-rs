use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fmt::Display;

pub mod v1;
pub mod v2;

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

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedPaymentKind {
    pub x402_version: u8,
    pub scheme: String,
    pub network: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<serde_json::Value>,
}
