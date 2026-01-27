//! Wire format types for EVM chain interactions.
//!
//! This module provides types that handle serialization and deserialization
//! of EVM-specific values in the x402 protocol wire format.

use alloy_primitives::{Address, U256, hex};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

/// An Ethereum address that serializes with EIP-55 checksum encoding.
///
/// This wrapper ensures addresses are always serialized in checksummed format
/// (e.g., `0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045`) for compatibility
/// with the x402 protocol wire format.
///
/// # Example
///
/// ```
/// use x402_rs::chain::eip155::ChecksummedAddress;
///
/// let addr: ChecksummedAddress = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045".parse().unwrap();
/// assert_eq!(addr.to_string(), "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChecksummedAddress(pub Address);

impl FromStr for ChecksummedAddress {
    type Err = hex::FromHexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let address = Address::from_str(s)?;
        Ok(Self(address))
    }
}

impl Display for ChecksummedAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_checksum(None))
    }
}

impl Serialize for ChecksummedAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_checksum(None))
    }
}

impl<'de> Deserialize<'de> for ChecksummedAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl From<ChecksummedAddress> for Address {
    fn from(value: ChecksummedAddress) -> Self {
        value.0
    }
}

impl From<Address> for ChecksummedAddress {
    fn from(address: Address) -> Self {
        Self(address)
    }
}

impl PartialEq<ChecksummedAddress> for Address {
    fn eq(&self, other: &ChecksummedAddress) -> bool {
        self.eq(&other.0)
    }
}

/// A token amount represented as a U256, serialized as a decimal string.
///
/// This wrapper ensures token amounts are serialized as decimal strings
/// (e.g., `"1000000"`) rather than hex to maintain compatibility with
/// the x402 protocol wire format and avoid precision issues in JSON.
///
/// # Example
///
/// ```
/// use x402_rs::chain::eip155::TokenAmount;
/// use alloy_primitives::U256;
///
/// let amount = TokenAmount(U256::from(1_000_000u64));
/// let json = serde_json::to_string(&amount).unwrap();
/// assert_eq!(json, "\"1000000\"");
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenAmount(pub U256);

impl FromStr for TokenAmount {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let u256 = U256::from_str_radix(s, 10).map_err(|_| "invalid token amount".to_string())?;
        Ok(Self(u256))
    }
}

impl Serialize for TokenAmount {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for TokenAmount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl From<TokenAmount> for U256 {
    fn from(value: TokenAmount) -> Self {
        value.0
    }
}

impl From<U256> for TokenAmount {
    fn from(value: U256) -> Self {
        Self(value)
    }
}

impl From<u128> for TokenAmount {
    fn from(value: u128) -> Self {
        Self(U256::from(value))
    }
}

impl From<u64> for TokenAmount {
    fn from(value: u64) -> Self {
        Self(U256::from(value))
    }
}
