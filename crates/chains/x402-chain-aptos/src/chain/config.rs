use serde::{Deserialize, Serialize};
use std::ops::Deref;
use std::str::FromStr;
use url::Url;
use x402_types::chain::ChainId;
use x402_types::config::LiteralOrEnv;

use crate::chain::AptosChainReference;

#[derive(Debug, Clone)]
pub struct AptosChainConfig {
    pub chain_reference: AptosChainReference,
    pub inner: AptosChainConfigInner,
}

impl AptosChainConfig {
    pub fn signer(&self) -> Option<&AptosSignerConfig> {
        self.inner.signer.as_ref()
    }
    pub fn rpc(&self) -> &Url {
        self.inner.rpc.inner()
    }
    pub fn api_key(&self) -> Option<&str> {
        self.inner.api_key.as_ref().map(|k| k.inner().as_str())
    }
    pub fn sponsor_gas(&self) -> bool {
        *self.inner.sponsor_gas.inner()
    }
    pub fn chain_reference(&self) -> AptosChainReference {
        self.chain_reference
    }
    pub fn chain_id(&self) -> ChainId {
        self.chain_reference.into()
    }
}

/// Configuration specific to Aptos chains.
///
/// # Example - Using environment variables (recommended for deployments)
///
/// ```toml
/// [aptos."aptos:1"]
/// rpc = "$APTOS_RPC_URL"
/// api_key = "$APTOS_API_KEY"
/// sponsor_gas = "$APTOS_SPONSOR_GAS"
/// signer = { private_key = "$APTOS_PRIVATE_KEY" }
/// ```
///
/// Set these environment variables:
/// - `APTOS_RPC_URL="https://fullnode.mainnet.aptoslabs.com/v1"`
/// - `APTOS_API_KEY="your-api-key"` (optional, sent as Bearer token)
/// - `APTOS_SPONSOR_GAS="true"`
/// - `APTOS_PRIVATE_KEY="0x..."`
///
/// # Example - Literal values in config
///
/// ```toml
/// [aptos."aptos:1"]
/// rpc = "https://fullnode.mainnet.aptoslabs.com/v1"
/// sponsor_gas = true
/// signer = { private_key = "$APTOS_PRIVATE_KEY" }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AptosChainConfigInner {
    /// RPC provider URL for this chain (required).
    /// Supports literal URLs or environment variable references like "$APTOS_RPC_URL".
    pub rpc: LiteralOrEnv<Url>,
    /// Optional API key for authenticated RPC access (e.g., Geomi nodes).
    /// If provided, sent as `Authorization: Bearer {api_key}` header with all RPC requests.
    /// Supports literal strings or environment variable references like "$APTOS_API_KEY".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_key: Option<LiteralOrEnv<String>>,
    /// Signer configuration for this chain (optional, required only if sponsor_gas is true).
    /// A hex-encoded private key (32 or 64 bytes) or env var reference like "$APTOS_PRIVATE_KEY".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<AptosSignerConfig>,
    /// Whether the facilitator should sponsor gas fees for transactions (default: false).
    /// If true, facilitator signs as fee payer and pays gas. If false, users pay their own gas.
    /// Supports literal booleans or environment variable references like "$APTOS_SPONSOR_GAS".
    #[serde(default = "aptos_chain_config::default_sponsor_gas")]
    pub sponsor_gas: LiteralOrEnv<bool>,
}

mod aptos_chain_config {
    use super::LiteralOrEnv;

    pub fn default_sponsor_gas() -> LiteralOrEnv<bool> {
        // Default to false when field is missing
        LiteralOrEnv::from_literal(false)
    }
}

// ============================================================================
// Aptos Private Key
// ============================================================================

/// A validated Aptos private key (32 or 64 bytes).
///
/// This type represents an Aptos private key which can be either:
/// - 32 bytes: Ed25519 seed
/// - 64 bytes: Full Ed25519 keypair (seed + public key)
///
/// The key is stored and parsed as a hex-encoded string with 0x prefix.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AptosPrivateKey(Vec<u8>);

impl AptosPrivateKey {
    /// Parse a hex string into a private key.
    pub fn from_hex(s: &str) -> Result<Self, String> {
        let s = s.strip_prefix("0x").unwrap_or(s);
        let bytes = hex::decode(s).map_err(|e| format!("Invalid hex: {}", e))?;

        if bytes.len() != 32 && bytes.len() != 64 {
            return Err(format!(
                "Private key must be 32 or 64 bytes, got {} bytes",
                bytes.len()
            ));
        }

        Ok(Self(bytes))
    }

    /// Encode the private key as hex.
    pub fn to_hex(&self) -> String {
        format!("0x{}", hex::encode(&self.0))
    }
}

impl Serialize for AptosPrivateKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl FromStr for AptosPrivateKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
    }
}

impl std::fmt::Display for AptosPrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Type alias for Aptos signer configuration.
///
/// Uses `LiteralOrEnv` to support both literal hex keys and environment variable references.
///
/// Example JSON:
/// ```json
/// {
///   "signer": "$APTOS_FACILITATOR_KEY"
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AptosSignerConfig(LiteralOrEnv<AptosPrivateKey>);

impl Deref for AptosSignerConfig {
    type Target = AptosPrivateKey;

    fn deref(&self) -> &Self::Target {
        self.0.inner()
    }
}
