use alloy_primitives::B256;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use x402_types::chain::ChainId;
use x402_types::config::{LiteralOrEnv, RpcConfig};

use crate::chain::Eip155ChainReference;

/// Configuration for an EVM-compatible chain in the x402 facilitator.
///
/// This struct combines a chain reference with chain-specific configuration
/// including RPC endpoints, signers, and network capabilities.
///
/// # Example
///
/// ```ignore
/// use x402_chain_eip155::chain::{Eip155ChainConfig, Eip155ChainReference};
///
/// let config = Eip155ChainConfig {
///     chain_reference: Eip155ChainReference::new(8453), // Base
///     inner: config_inner,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct Eip155ChainConfig {
    /// The numeric chain ID for this EVM network.
    pub chain_reference: Eip155ChainReference,
    /// Chain-specific configuration details.
    pub inner: Eip155ChainConfigInner,
}

impl Eip155ChainConfig {
    /// Returns the CAIP-2 chain ID for this configuration.
    pub fn chain_id(&self) -> ChainId {
        self.chain_reference.into()
    }
    /// Returns whether this chain supports EIP-1559 gas pricing.
    pub fn eip1559(&self) -> bool {
        self.inner.eip1559
    }

    /// Returns whether this chain supports flashblocks (immediate block finality).
    pub fn flashblocks(&self) -> bool {
        self.inner.flashblocks
    }

    /// Returns the transaction receipt timeout in seconds.
    pub fn receipt_timeout_secs(&self) -> u64 {
        self.inner.receipt_timeout_secs
    }

    /// Returns the signer configuration for this chain.
    pub fn signers(&self) -> &Eip155SignersConfig {
        &self.inner.signers
    }

    /// Returns the RPC endpoint configurations for this chain.
    pub fn rpc(&self) -> &Vec<RpcConfig> {
        &self.inner.rpc
    }

    /// Returns the numeric chain reference.
    pub fn chain_reference(&self) -> Eip155ChainReference {
        self.chain_reference
    }
}

/// Configuration specific to EVM-compatible chains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip155ChainConfigInner {
    /// Whether the chain supports EIP-1559 gas pricing.
    #[serde(default = "eip155_chain_config::default_eip1559")]
    pub eip1559: bool,
    /// Whether the chain supports flashblocks.
    #[serde(default = "eip155_chain_config::default_flashblocks")]
    pub flashblocks: bool,
    /// Signer configuration for this chain (required).
    /// Array of private keys (hex format) or env var references.
    pub signers: Eip155SignersConfig,
    /// RPC provider configuration for this chain (required).
    pub rpc: Vec<RpcConfig>,
    /// How long to wait till the transaction receipt is available (optional)
    #[serde(default = "eip155_chain_config::default_receipt_timeout_secs")]
    pub receipt_timeout_secs: u64,
}

mod eip155_chain_config {
    pub fn default_eip1559() -> bool {
        true
    }
    pub fn default_flashblocks() -> bool {
        false
    }
    pub fn default_receipt_timeout_secs() -> u64 {
        30
    }
}

/// Configuration for EVM signers.
///
/// Deserializes an array of private key strings (hex format, 0x-prefixed) and
/// validates them as valid 32-byte private keys. The `EthereumWallet` is created
/// lazily when needed via the `wallet()` method.
///
/// Each string can be:
/// - A literal hex private key: `"0xcafe..."`
/// - An environment variable reference: `"$PRIVATE_KEY"` or `"${PRIVATE_KEY}"`
///
/// Example JSON:
/// ```json
/// {
///   "signers": [
///     "$HOT_WALLET_KEY",
///     "0xcafe000000000000000000000000000000000000000000000000000000000001"
///   ]
/// }
/// ```
pub type Eip155SignersConfig = Vec<LiteralOrEnv<EvmPrivateKey>>;

// ============================================================================
// EVM Private Key
// ============================================================================

/// A validated EVM private key (32 bytes).
///
/// This type represents a raw private key that has been validated as a proper
/// 32-byte hex value. It can be converted to a `PrivateKeySigner` when needed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct EvmPrivateKey(B256);

impl EvmPrivateKey {
    /// Get the raw 32 bytes of the private key.
    pub fn as_bytes(&self) -> &[u8; 32] {
        self.0.as_ref()
    }
}

impl PartialEq for EvmPrivateKey {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

impl FromStr for EvmPrivateKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        B256::from_str(s)
            .map(Self)
            .map_err(|e| format!("Invalid evm private key: {}", e))
    }
}
