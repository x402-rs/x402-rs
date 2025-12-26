use alloy_primitives::{Address, U256, hex};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::str::FromStr;

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

/// U256 - serialized as decimal string
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TokenAmount(pub U256);

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
        let u256 = U256::from_str_radix(&s, 10).map_err(serde::de::Error::custom)?;
        Ok(TokenAmount(u256))
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
