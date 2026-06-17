//! Configuration types for TRON chain providers.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;
use url::Url;
use x402_types::chain::ChainId;
use x402_types::config::LiteralOrEnv;

use crate::chain::{TronAddress, TronChainReference};

/// Full configuration for a TRON chain provider.
#[derive(Debug, Clone)]
pub struct TronChainConfig {
    /// The TRON chain reference (e.g. `0x2b6653dc` for mainnet).
    pub chain_reference: TronChainReference,
    /// Chain-specific inner configuration.
    pub inner: TronChainConfigInner,
}

impl TronChainConfig {
    /// Returns the CAIP-2 chain ID for this configuration.
    pub fn chain_id(&self) -> ChainId {
        self.chain_reference.chain_id()
    }
}

/// Inner configuration details for a TRON chain.
///
/// Example JSON:
/// ```json
/// {
///   "rpc_url": "https://api.trongrid.io",
///   "signers": ["$TRON_FACILITATOR_KEY"],
///   "permit2_proxy_address": "TTJxU3P8rHycAyFY4kVtGNfmnMH4ezcuM9"
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TronChainConfigInner {
    /// TronGrid HTTP API base URL (literal or env var reference).
    pub rpc_url: LiteralOrEnv<Url>,
    /// One or more facilitator signing keys (hex format, `0x`-prefixed or env var references).
    pub signers: TronSignersConfig,
    /// Optional contract addresses in Base58Check format.
    /// If not set, the well-known deployment for this network is used (if any).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contracts: Option<TronContracts>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TronContracts {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sun_permit2: Option<TronAddress>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x402_exact_permit2_proxy: Option<TronAddress>,
}

// ── TronSignersConfig ────────────────────────────────────────────────────────

/// One or more TRON facilitator signing keys.
///
/// Each entry can be:
/// - A literal hex private key: `"0xcafe..."` or `"cafe..."`
/// - An environment variable reference: `"$TRON_KEY"` or `"${TRON_KEY}"`
pub type TronSignersConfig = Vec<LiteralOrEnv<TronPrivateKey>>;

// ── TronPrivateKey ───────────────────────────────────────────────────────────

/// A validated TRON private key (32 bytes, secp256k1).
///
/// Stored as raw bytes and parsed from a hex string (with or without `0x` prefix).
#[derive(Clone, PartialEq, Eq)]
pub struct TronPrivateKey(k256::ecdsa::SigningKey);

impl TronPrivateKey {
    pub fn new(key: k256::ecdsa::SigningKey) -> Self {
        Self(key)
    }
}

impl From<TronPrivateKey> for k256::ecdsa::SigningKey {
    fn from(key: TronPrivateKey) -> Self {
        key.0
    }
}

impl fmt::Debug for TronPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("TronPrivateKey([REDACTED])")
    }
}

impl FromStr for TronPrivateKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let lowercase = s.to_lowercase();
        let hex = lowercase.strip_prefix("0x").unwrap_or(s);
        let bytes = alloy_primitives::hex::decode(hex)
            .map_err(|e| format!("Invalid TRON private key hex: {e}"))?;
        // Validate it's a usable secp256k1 key by attempting to construct one.
        let key = k256::ecdsa::SigningKey::from_slice(&bytes)
            .map_err(|e| format!("Invalid secp256k1 private key: {e}"))?;
        Ok(Self(key))
    }
}

impl Serialize for TronPrivateKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let as_string = self.to_string();
        serializer.serialize_str(&as_string)
    }
}

impl<'de> Deserialize<'de> for TronPrivateKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for TronPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.0.to_bytes();
        let as_hex = alloy_primitives::hex::encode(&bytes);
        write!(f, "{}", as_hex)
    }
}
