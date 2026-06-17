//! TRON address type with Base58Check encoding.

use alloy_primitives::Address;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;

/// A TRON address in Base58Check format (the standard "T..." representation).
///
/// Internally holds the 20-byte EVM address payload (same key derivation as Ethereum).
/// Serializes as Base58Check for the x402 wire format and for TronGrid API calls.
/// Use `Into<Address>` / `From<TronAddress>` to get the `alloy` `Address` needed for
/// EIP-712 / TIP-712 signing.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub struct TronAddress(pub [u8; 20]);

impl TronAddress {
    /// Creates a `TronAddress` from raw 20 bytes (EVM address bytes).
    pub fn from_bytes(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }

    /// Decodes a Base58Check string into a `TronAddress`.
    pub fn from_base58(s: &str) -> Result<Self, TronAddressError> {
        let decoded = bs58::decode(s)
            .into_vec()
            .map_err(|_| TronAddressError::InvalidBase58)?;

        // Must be 25 bytes: 1 prefix + 20 address + 4 checksum
        if decoded.len() != 25 {
            return Err(TronAddressError::InvalidLength(decoded.len()));
        }

        // Verify checksum (SHA256(SHA256(payload))[0..4])
        let payload = &decoded[..21];
        let checksum = &decoded[21..25];
        let hash1 = Sha256::digest(payload);
        let hash2 = Sha256::digest(&hash1);
        if &hash2[..4] != checksum {
            return Err(TronAddressError::InvalidChecksum);
        }

        // First byte must be 0x41 (TRON address prefix)
        if decoded[0] != 0x41 {
            return Err(TronAddressError::InvalidPrefix(decoded[0]));
        }

        let mut bytes = [0u8; 20];
        bytes.copy_from_slice(&decoded[1..21]);
        Ok(Self(bytes))
    }

    /// Encodes the address to Base58Check.
    pub fn as_base58(&self) -> String {
        let mut payload = [0u8; 21];
        payload[0] = 0x41;
        payload[1..21].copy_from_slice(&self.0);

        let hash1 = Sha256::digest(&payload);
        let hash2 = Sha256::digest(&hash1);

        let mut full = [0u8; 25];
        full[..21].copy_from_slice(&payload);
        full[21..25].copy_from_slice(&hash2[..4]);

        bs58::encode(full).into_string()
    }
}

impl fmt::Display for TronAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_base58())
    }
}

impl FromStr for TronAddress {
    type Err = TronAddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_base58(s)
    }
}

impl TryFrom<&str> for TronAddress {
    type Error = TronAddressError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::from_str(value)
    }
}

impl Serialize for TronAddress {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.as_base58())
    }
}

impl<'de> Deserialize<'de> for TronAddress {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        TronAddress::from_base58(&s).map_err(serde::de::Error::custom)
    }
}

impl From<Address> for TronAddress {
    fn from(addr: Address) -> Self {
        Self(*addr.as_ref())
    }
}

impl From<&Address> for TronAddress {
    fn from(addr: &Address) -> Self {
        Self(*addr.as_ref())
    }
}

impl From<TronAddress> for Address {
    fn from(addr: TronAddress) -> Self {
        Address::from(addr.0)
    }
}

/// Errors that can occur when parsing a TRON address.
#[derive(Debug, thiserror::Error)]
pub enum TronAddressError {
    #[error("Invalid Base58 encoding")]
    InvalidBase58,
    #[error("Invalid address length: expected 25 bytes, got {0}")]
    InvalidLength(usize),
    #[error("Invalid checksum")]
    InvalidChecksum,
    #[error("Invalid address prefix: expected 0x41, got 0x{0:02x}")]
    InvalidPrefix(u8),
}
