use std::str::FromStr;
use alloy_primitives::{hex, Address};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChecksummedAddress(pub Address);

impl FromStr for crate::scheme::v1_eip155_exact::ChecksummedAddress {
    type Err = hex::FromHexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let address = Address::from_str(s)?;
        Ok(Self(address))
    }
}

impl Serialize for crate::scheme::v1_eip155_exact::ChecksummedAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_checksum(None))
    }
}

impl<'de> Deserialize<'de> for crate::scheme::v1_eip155_exact::ChecksummedAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl Into<Address> for crate::scheme::v1_eip155_exact::ChecksummedAddress {
    fn into(self) -> Address {
        self.0
    }
}

impl From<Address> for crate::scheme::v1_eip155_exact::ChecksummedAddress {
    fn from(address: Address) -> Self {
        Self(address)
    }
}
