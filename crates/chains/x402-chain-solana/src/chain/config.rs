use serde::{Deserialize, Serialize};
use solana_client::client_error::reqwest::Url;
use std::ops::Deref;
use std::str::FromStr;
use x402_types::chain::ChainId;
use x402_types::config::LiteralOrEnv;

use crate::chain::SolanaChainReference;

/// Configuration for a Solana chain in the x402 facilitator.
///
/// This struct combines a chain reference with chain-specific configuration
/// including RPC endpoints, signer, and compute budget parameters.
///
/// # Example
///
/// ```ignore
/// use x402_chain_solana::chain::{SolanaChainConfig, SolanaChainReference};
///
/// let config = SolanaChainConfig {
///     chain_reference: SolanaChainReference::solana(),
///     inner: config_inner,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct SolanaChainConfig {
    /// The Solana network identifier (genesis hash prefix).
    pub chain_reference: SolanaChainReference,
    /// Chain-specific configuration details.
    pub inner: SolanaChainConfigInner,
}

impl SolanaChainConfig {
    /// Returns the signer configuration for this chain.
    pub fn signer(&self) -> &SolanaSignerConfig {
        &self.inner.signer
    }
    /// Returns the RPC endpoint URL for this chain.
    pub fn rpc(&self) -> &Url {
        &self.inner.rpc
    }

    /// Returns the maximum compute unit limit for transactions.
    pub fn max_compute_unit_limit(&self) -> u32 {
        self.inner.max_compute_unit_limit
    }

    /// Returns the maximum compute unit price (in micro-lamports).
    pub fn max_compute_unit_price(&self) -> u64 {
        self.inner.max_compute_unit_price
    }

    /// Returns the chain reference (genesis hash prefix).
    pub fn chain_reference(&self) -> SolanaChainReference {
        self.chain_reference
    }

    /// Returns the CAIP-2 chain ID for this configuration.
    pub fn chain_id(&self) -> ChainId {
        self.chain_reference.into()
    }

    /// Returns the optional WebSocket pubsub endpoint URL.
    pub fn pubsub(&self) -> &Option<Url> {
        &self.inner.pubsub
    }
}

/// Configuration specific to Solana chains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaChainConfigInner {
    /// Signer configuration for this chain (required).
    /// A single private key (base58 format, 64 bytes) or env var reference.
    pub signer: SolanaSignerConfig,
    /// RPC provider configuration for this chain (required).
    pub rpc: Url,
    /// RPC pubsub provider endpoint (optional)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pubsub: Option<Url>,
    /// Maximum compute unit limit for transactions (optional)
    #[serde(default = "solana_chain_config::default_max_compute_unit_limit")]
    pub max_compute_unit_limit: u32,
    /// Maximum compute unit price for transactions (optional)
    #[serde(default = "solana_chain_config::default_max_compute_unit_price")]
    pub max_compute_unit_price: u64,
}

mod solana_chain_config {
    pub fn default_max_compute_unit_limit() -> u32 {
        400_000
    }
    pub fn default_max_compute_unit_price() -> u64 {
        1_000_000
    }
}

// ============================================================================
// Solana Private Key
// ============================================================================

/// A validated Solana private key (64 bytes in standard Solana format).
///
/// This type represents a standard Solana keypair in its 64-byte format:
/// - First 32 bytes: the Ed25519 secret key (seed)
/// - Last 32 bytes: the Ed25519 public key
///
/// The key is stored and parsed as a base58-encoded 64-byte array,
/// which is the standard format used by Solana CLI and wallets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SolanaPrivateKey([u8; 64]);

impl SolanaPrivateKey {
    /// Parse a base58 string into a private key (64 bytes in standard Solana format).
    ///
    /// The standard Solana keypair format is 64 bytes:
    /// - First 32 bytes: secret key (seed)
    /// - Last 32 bytes: public key
    pub fn from_base58(s: &str) -> Result<Self, String> {
        let bytes = bs58::decode(s)
            .into_vec()
            .map_err(|e| format!("Invalid base58: {}", e))?;

        if bytes.len() != 64 {
            return Err(format!(
                "Private key must be 64 bytes (standard Solana format), got {} bytes",
                bytes.len()
            ));
        }

        let mut arr = [0u8; 64];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    /// Encode the keypair back to base58.
    pub fn to_base58(&self) -> String {
        bs58::encode(&self.0).into_string()
    }
}

impl Serialize for SolanaPrivateKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_base58())
    }
}

impl FromStr for SolanaPrivateKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_base58(s)
    }
}

impl std::fmt::Display for SolanaPrivateKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.to_base58())
    }
}

/// Type alias for Solana signer configuration.
///
/// Uses `LiteralOrEnv` to support both literal base58 keys and environment variable references.
///
/// Example JSON:
/// ```json
/// {
///   "signer": "$SOLANA_FACILITATOR_KEY"
/// }
/// ```
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SolanaSignerConfig(LiteralOrEnv<SolanaPrivateKey>);

impl Deref for SolanaSignerConfig {
    type Target = SolanaPrivateKey;

    fn deref(&self) -> &Self::Target {
        self.0.inner()
    }
}
