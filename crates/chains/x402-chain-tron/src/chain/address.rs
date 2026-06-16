//! TRON address type with Base58Check encoding.
//!
//! TRON addresses have three representations:
//! - Base58Check (wire format): "T..." (e.g., "TXyz...")
//! - TRON hex: "41" + 20-byte-hex (42 hex chars)
//! - EVM hex: "0x" + 20-byte-hex (for EIP-712 signing)

use alloy_primitives::Address;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};
use std::fmt;
use std::str::FromStr;

/// A TRON address in Base58Check encoding.
///
/// Serializes and deserializes as Base58 (the "T..." format used on the wire).
/// Internally holds the 20-byte EVM address payload.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TronAddress(pub [u8; 20]);

impl TronAddress {
    /// Creates a TronAddress from raw 20 bytes (EVM address bytes).
    pub fn from_bytes(bytes: [u8; 20]) -> Self {
        Self(bytes)
    }

    /// Creates a TronAddress from an alloy `Address`.
    pub fn from_evm_address(addr: Address) -> Self {
        Self(*addr.as_ref())
    }

    /// Returns the alloy `Address` (EVM hex).
    pub fn to_evm_address(&self) -> Address {
        Address::from(self.0)
    }

    /// Returns the TRON hex representation: "41" + 40 hex chars.
    pub fn to_tron_hex(&self) -> String {
        let mut result = String::with_capacity(42);
        result.push_str("41");
        result.push_str(&hex::encode(self.0));
        result
    }

    /// Decodes a Base58Check string into a TronAddress.
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
    pub fn to_base58(&self) -> String {
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

    /// Decodes a TRON hex string ("41" + 40 hex chars) into a TronAddress.
    pub fn from_tron_hex(s: &str) -> Result<Self, TronAddressError> {
        let hex_str = s.strip_prefix("0x").unwrap_or(s);
        if hex_str.len() != 42 {
            return Err(TronAddressError::InvalidTronHex);
        }
        let bytes = hex::decode(hex_str).map_err(|_| TronAddressError::InvalidTronHex)?;
        if bytes[0] != 0x41 {
            return Err(TronAddressError::InvalidPrefix(bytes[0]));
        }
        let mut addr = [0u8; 20];
        addr.copy_from_slice(&bytes[1..21]);
        Ok(Self(addr))
    }
}

impl fmt::Display for TronAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_base58())
    }
}

impl FromStr for TronAddress {
    type Err = TronAddressError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_base58(s)
    }
}

impl Serialize for TronAddress {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_base58())
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
        Self::from_evm_address(addr)
    }
}

impl From<TronAddress> for Address {
    fn from(addr: TronAddress) -> Self {
        addr.to_evm_address()
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
    #[error("Invalid TRON hex format")]
    InvalidTronHex,
}

/// Hex encoding helper (avoid pulling in another crate — use alloy_primitives hex).
mod hex {
    pub fn encode(bytes: impl AsRef<[u8]>) -> String {
        alloy_primitives::hex::encode(bytes.as_ref())
    }

    pub fn decode(s: &str) -> Result<Vec<u8>, ()> {
        alloy_primitives::hex::decode(s).map_err(|_| ())
    }
}
