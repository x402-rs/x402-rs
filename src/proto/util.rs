//! Utility types for protocol serialization.
//!
//! This module provides helper types for serializing values in the x402 wire format.

use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::str::FromStr;

/// A `u64` value that serializes as a string.
///
/// Some JSON parsers (particularly in JavaScript) cannot accurately represent
/// large integers. This type serializes `u64` values as strings to preserve
/// precision across all platforms.
///
/// # Example
///
/// ```rust
/// use x402_rs::proto::util::U64String;
///
/// let value = U64String::from(12345678901234567890u64);
/// let json = serde_json::to_string(&value).unwrap();
/// assert_eq!(json, "\"12345678901234567890\"");
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct U64String(u64);

impl U64String {
    /// Returns the inner `u64` value.
    pub fn inner(&self) -> u64 {
        self.0
    }
}

impl FromStr for U64String {
    type Err = <u64 as FromStr>::Err;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        s.parse::<u64>().map(Self)
    }
}

impl From<u64> for U64String {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

impl From<U64String> for u64 {
    fn from(value: U64String) -> Self {
        value.0
    }
}

impl Serialize for U64String {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for U64String {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse::<u64>().map(Self).map_err(D::Error::custom)
    }
}
