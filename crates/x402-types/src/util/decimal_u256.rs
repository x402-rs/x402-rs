use alloy_primitives::U256;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fmt::Display;

/// A `U256` amount that serializes/deserializes as a decimal string.
///
/// The x402 V2 wire format encodes payment amounts as decimal strings
/// (e.g., `"10000"` for 10000 token units). Alloy's default `U256` serde
/// implementation uses hex encoding, so this newtype provides correct
/// decimal-string handling for V2 payment requirements.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DecimalU256(pub U256);

impl From<DecimalU256> for U256 {
    fn from(v: DecimalU256) -> Self {
        v.0
    }
}

impl Display for DecimalU256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<U256> for DecimalU256 {
    fn from(v: U256) -> Self {
        Self(v)
    }
}

impl Serialize for DecimalU256 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for DecimalU256 {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct DecimalU256Visitor;
        impl<'de> serde::de::Visitor<'de> for DecimalU256Visitor {
            type Value = DecimalU256;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a decimal string or integer representing a U256 amount")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<DecimalU256, E> {
                U256::from_str_radix(v, 10)
                    .map(DecimalU256)
                    .map_err(|e| E::custom(format!("invalid decimal U256: {e}")))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<DecimalU256, E> {
                Ok(DecimalU256(U256::from(v)))
            }
            fn visit_u128<E: serde::de::Error>(self, v: u128) -> Result<DecimalU256, E> {
                Ok(DecimalU256(U256::from(v)))
            }
        }
        deserializer.deserialize_any(DecimalU256Visitor)
    }
}

pub mod decimal_u256 {
    use alloy_primitives::U256;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialize a U256 as a decimal string.
    pub fn serialize<S>(value: &U256, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    /// Deserialize a decimal string into a U256.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<U256, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        U256::from_str_radix(&s, 10).map_err(serde::de::Error::custom)
    }
}
