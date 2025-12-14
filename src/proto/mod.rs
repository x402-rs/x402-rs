use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fmt::Display;

use crate::proto::v1::X402Version1;
use crate::proto::v2::X402Version2;

pub mod v1;
pub mod v2;

/// Represents the protocol version. Versions 1 and 2 are supported.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum X402Version {
    /// Version `1`.
    V1(X402Version1),
    /// Version `2`.
    V2(X402Version2),
}

impl X402Version {
    pub fn v1() -> X402Version {
        X402Version::V1(X402Version1)
    }
    pub fn v2() -> X402Version {
        X402Version::V2(X402Version2)
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
pub struct X402VersionError(pub u8);

impl TryFrom<u8> for X402Version {
    type Error = X402VersionError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            X402Version1::VALUE => Ok(X402Version::v1()),
            X402Version2::VALUE => Ok(X402Version::v2()),
            _ => Err(X402VersionError(value)),
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
