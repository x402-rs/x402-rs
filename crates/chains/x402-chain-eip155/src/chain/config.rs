use alloy_primitives::B256;
use serde::{Deserialize, Serialize};
use std::num::NonZeroU64;
use std::str::FromStr;
use url::Url;
use x402_types::chain::ChainId;
use x402_types::config::LiteralOrEnv;

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

    /// Returns whether a settle request whose ERC-3009 authorization is already used is
    /// answered with the transaction that used it.
    pub fn reconcile_used_authorizations(&self) -> bool {
        self.inner.reconcile_used_authorizations
    }

    /// Returns the most blocks one `eth_getLogs` call of the used-authorization lookup covers.
    pub fn log_query_max_blocks(&self) -> NonZeroU64 {
        self.inner.log_query_max_blocks
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
    /// Whether a settle request with no pending-settlement entry, whose ERC-3009 authorization
    /// is already used, is answered with the transaction that used it (optional, off by
    /// default).
    ///
    /// When on, such a request is looked up on chain: if one of this chain's signers sent a
    /// transaction in the last five minutes (searched with at most 50 `eth_getLogs` calls of
    /// `log_query_max_blocks` blocks each, newest first) that used the
    /// authorization and transferred the signed value from the payer to the recipient,
    /// `/settle` answers success with its hash instead of failing on the used authorization.
    /// This answers the retry of a settlement that reached another instance, or this one after
    /// a restart. It also makes `/settle` answer success again, for those five minutes, to any
    /// other request that carries a settled authorization and matches its requirements.
    /// Calling `/verify` first protects a resource server only from a copy of the payment that
    /// arrives after the settlement has landed, because verifying a used authorization fails. A
    /// copy verified before then that reaches `/settle` after it (one sent while the first
    /// copy's handler is still running, or at the same time) is answered with the first copy's
    /// transaction, so the server serves that copy too. Turn it on only when the resource
    /// servers do not take a `/settle` success alone as proof of a new payment.
    #[serde(default)]
    pub reconcile_used_authorizations: bool,
    /// Most blocks one `eth_getLogs` call of the `reconcile_used_authorizations` lookup covers
    /// (optional, default 100). Set it to the block range limit of the chain's RPCs (the
    /// smallest, if they differ): a larger value needs fewer calls. A range an RPC refuses is
    /// split in halves until it is accepted, and the smaller size is kept for the rest of the
    /// lookup, so a lower limit costs a few refused calls per lookup.
    #[serde(default = "eip155_chain_config::default_log_query_max_blocks")]
    pub log_query_max_blocks: NonZeroU64,
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
    pub fn default_log_query_max_blocks() -> std::num::NonZeroU64 {
        crate::chain::authorization_use::LOG_QUERY_MAX_BLOCKS
    }
}

/// RPC provider configuration for a single provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcConfig {
    /// HTTP URL for the RPC endpoint.
    pub http: LiteralOrEnv<Url>,
    /// Rate limit for requests per second (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config(extra: &str) -> Eip155ChainConfigInner {
        serde_json::from_str(&format!(
            r#"{{
                "signers": ["0xcafe000000000000000000000000000000000000000000000000000000000001"],
                "rpc": [{{"http": "https://sepolia.base.org"}}]{extra}
            }}"#
        ))
        .unwrap()
    }

    #[test]
    fn test_reconcile_used_authorizations_is_off_unless_set() {
        assert!(!config("").reconcile_used_authorizations);
        assert!(config(r#", "reconcile_used_authorizations": true"#).reconcile_used_authorizations);
    }

    #[test]
    fn test_log_query_max_blocks_is_100_unless_set_and_not_zero() {
        assert_eq!(config("").log_query_max_blocks.get(), 100);
        assert_eq!(
            config(r#", "log_query_max_blocks": 200"#)
                .log_query_max_blocks
                .get(),
            200
        );
        let zero = serde_json::from_str::<Eip155ChainConfigInner>(
            r#"{
                "signers": ["0xcafe000000000000000000000000000000000000000000000000000000000001"],
                "rpc": [{"http": "https://sepolia.base.org"}],
                "log_query_max_blocks": 0
            }"#,
        );
        assert!(zero.is_err());
    }
}
