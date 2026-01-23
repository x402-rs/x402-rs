//! Configuration module for the x402 facilitator server.

use alloy_primitives::B256;
use alloy_primitives::hex;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::IpAddr;
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use url::Url;

use crate::chain::aptos;
use crate::chain::eip155;
use crate::chain::solana;
use crate::chain::{ChainId, ChainIdPattern};

/// CLI arguments for the x402 facilitator server.
#[derive(Parser, Debug)]
#[command(name = "x402-rs")]
#[command(about = "x402 Facilitator HTTP server")]
struct CliArgs {
    /// Path to the JSON configuration file
    #[arg(long, short, env = "CONFIG", default_value = "config.json")]
    config: PathBuf,
}

/// Server configuration.
///
/// Fields use serde defaults that fall back to environment variables,
/// then to hardcoded defaults.
#[derive(Debug, Clone, Deserialize)]
pub struct Config<TChainsConfig = ChainsConfig> {
    #[serde(default = "config_defaults::default_port")]
    port: u16,
    #[serde(default = "config_defaults::default_host")]
    host: IpAddr,
    #[serde(default)]
    chains: TChainsConfig,
    #[serde(default)]
    schemes: Vec<SchemeConfig>,
}

/// Configuration for a specific scheme.
///
/// Each scheme entry specifies which scheme to use and which chains it applies to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SchemeConfig {
    /// Whether this scheme is enabled (defaults to true).
    #[serde(default = "scheme_config_defaults::default_enabled")]
    pub enabled: bool,
    /// The scheme id (e.g., "v1-eip155-exact").
    pub id: String,
    /// The chain pattern this scheme applies to (e.g., "eip155:84532", "eip155:*", "eip155:{1,8453}").
    pub chains: ChainIdPattern,
    /// Scheme-specific configuration (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub config: Option<serde_json::Value>,
}

mod scheme_config_defaults {
    pub fn default_enabled() -> bool {
        true
    }
}

/// Configuration for a specific chain.
///
/// This enum represents chain-specific configuration that varies by chain family
/// (EVM vs Solana vs Aptos). The chain family is determined by the CAIP-2 prefix of the
/// chain identifier key (e.g., "eip155:" for EVM, "solana:" for Solana, "aptos:" for Aptos).
#[derive(Debug, Clone)]
pub enum ChainConfig {
    /// EVM chain configuration (for chains with "eip155:" prefix).
    Eip155(Box<Eip155ChainConfig>),
    /// Solana chain configuration (for chains with "solana:" prefix).
    Solana(Box<SolanaChainConfig>),
    /// Aptos chain configuration (for chains with "aptos:" prefix).
    Aptos(Box<AptosChainConfig>),
}

/// RPC provider configuration for a single provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcConfig {
    /// HTTP URL for the RPC endpoint.
    pub http: Url,
    /// Rate limit for requests per second (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,
}

// ============================================================================
// Environment Variable Resolution
// ============================================================================

/// A transparent wrapper that resolves environment variables during deserialization.
///
/// Supports both literal values and environment variable references:
/// - Literal: `"http://localhost:8083"`
/// - Simple env var: `"$TREASURY_URL"`
/// - Braced env var: `"${TREASURY_URL}"`
///
/// The wrapper implements `Deref` to provide transparent access to the inner type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiteralOrEnv<T>(T);

impl<T> LiteralOrEnv<T> {
    /// Get a reference to the inner value
    #[allow(dead_code)]
    pub fn inner(&self) -> &T {
        &self.0
    }

    /// Consume the wrapper and return the inner value
    #[allow(dead_code)]
    pub fn into_inner(self) -> T {
        self.0
    }

    /// Parse environment variable syntax from a string.
    /// Returns the variable name if the string matches `$VAR` or `${VAR}` syntax.
    fn parse_env_var_syntax(s: &str) -> Option<String> {
        if s.starts_with("${") && s.ends_with('}') {
            // ${VAR} syntax
            Some(s[2..s.len() - 1].to_string())
        } else if s.starts_with('$') && s.len() > 1 {
            // $VAR syntax - extract until first non-alphanumeric/underscore character
            let var_name = &s[1..];
            if var_name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                Some(var_name.to_string())
            } else {
                None
            }
        } else {
            None
        }
    }
}

impl<T> Deref for LiteralOrEnv<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for LiteralOrEnv<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<'de, T> Deserialize<'de> for LiteralOrEnv<T>
where
    T: FromStr,
    T::Err: std::fmt::Display,
{
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        // Check if it's an environment variable reference
        let value = if let Some(var_name) = Self::parse_env_var_syntax(&s) {
            std::env::var(&var_name).map_err(|_| {
                serde::de::Error::custom(format!(
                    "Environment variable '{}' not found (referenced as '{}')",
                    var_name, s
                ))
            })?
        } else {
            s
        };

        // Parse the value as type T
        let parsed = value
            .parse::<T>()
            .map_err(|e| serde::de::Error::custom(format!("Failed to parse value: {}", e)))?;

        Ok(LiteralOrEnv(parsed))
    }
}

impl<T> serde::Serialize for LiteralOrEnv<T>
where
    T: Serialize,
{
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

// impl<T: PartialEq> PartialEq for LiteralOrEnv<T> {
//     fn eq(&self, other: &Self) -> bool {
//         self.0 == other.0
//     }
// }

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

// ============================================================================
// EVM Signers Configuration
// ============================================================================

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

// ============================================================================
// Chain Configurations
// ============================================================================

#[derive(Debug, Clone)]
pub struct Eip155ChainConfig {
    pub chain_reference: eip155::Eip155ChainReference,
    pub inner: Eip155ChainConfigInner,
}

impl Eip155ChainConfig {
    pub fn chain_id(&self) -> ChainId {
        self.chain_reference.into()
    }
    pub fn eip1559(&self) -> bool {
        self.inner.eip1559
    }
    pub fn flashblocks(&self) -> bool {
        self.inner.flashblocks
    }
    pub fn receipt_timeout_secs(&self) -> u64 {
        self.inner.receipt_timeout_secs
    }
    pub fn signers(&self) -> &Eip155SignersConfig {
        &self.inner.signers
    }
    pub fn rpc(&self) -> &Vec<RpcConfig> {
        &self.inner.rpc
    }
    pub fn chain_reference(&self) -> eip155::Eip155ChainReference {
        self.chain_reference
    }
}

#[derive(Debug, Clone)]
pub struct SolanaChainConfig {
    pub chain_reference: solana::SolanaChainReference,
    pub inner: SolanaChainConfigInner,
}

impl SolanaChainConfig {
    pub fn signer(&self) -> &SolanaSignerConfig {
        &self.inner.signer
    }
    pub fn rpc(&self) -> &Url {
        &self.inner.rpc
    }
    pub fn max_compute_unit_limit(&self) -> u32 {
        self.inner.max_compute_unit_limit
    }
    pub fn max_compute_unit_price(&self) -> u64 {
        self.inner.max_compute_unit_price
    }
    pub fn chain_reference(&self) -> solana::SolanaChainReference {
        self.chain_reference
    }
    pub fn chain_id(&self) -> ChainId {
        self.chain_reference.into()
    }
    pub fn pubsub(&self) -> &Option<Url> {
        &self.inner.pubsub
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

// ============================================================================
// Aptos Chain Configuration
// ============================================================================

#[derive(Debug, Clone)]
pub struct AptosChainConfig {
    pub chain_reference: aptos::AptosChainReference,
    pub inner: AptosChainConfigInner,
}

impl AptosChainConfig {
    pub fn signer(&self) -> Option<&AptosSignerConfig> {
        self.inner.signer.as_ref()
    }
    pub fn rpc(&self) -> &Url {
        &self.inner.rpc
    }
    pub fn sponsor_gas(&self) -> bool {
        self.inner.sponsor_gas
    }
    pub fn chain_reference(&self) -> aptos::AptosChainReference {
        self.chain_reference
    }
    pub fn chain_id(&self) -> ChainId {
        self.chain_reference.into()
    }
}

/// Configuration specific to Aptos chains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AptosChainConfigInner {
    /// RPC provider URL for this chain (required).
    pub rpc: Url,
    /// Signer configuration for this chain (optional, required only if sponsor_gas is true).
    /// A hex-encoded private key (32 or 64 bytes) or env var reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signer: Option<AptosSignerConfig>,
    /// Whether the facilitator should sponsor gas fees for transactions (default: false).
    /// If true, facilitator signs as fee payer and pays gas. If false, users pay their own gas.
    #[serde(default)]
    pub sponsor_gas: bool,
}

/// Configuration for chains.
///
/// This is a wrapper around Vec<ChainConfig> that provides custom serialization
/// as a map where keys are CAIP-2 chain identifiers.
#[derive(Debug, Clone, Default)]
pub struct ChainsConfig(pub Vec<ChainConfig>);

impl Deref for ChainsConfig {
    type Target = Vec<ChainConfig>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for ChainsConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let chains = &self.0;
        let mut map = serializer.serialize_map(Some(chains.len()))?;
        for chain_config in chains {
            match chain_config {
                ChainConfig::Eip155(config) => {
                    let chain_id = config.chain_id();
                    let inner = &config.inner;
                    map.serialize_entry(&chain_id, inner)?;
                }
                ChainConfig::Solana(config) => {
                    let chain_id = config.chain_id();
                    let inner = &config.inner;
                    map.serialize_entry(&chain_id, inner)?;
                }
                ChainConfig::Aptos(config) => {
                    let chain_id = config.chain_id();
                    let inner = &config.inner;
                    map.serialize_entry(&chain_id, inner)?;
                }
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for ChainsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{MapAccess, Visitor};
        use std::fmt;

        struct ChainsVisitor;

        impl<'de> Visitor<'de> for ChainsVisitor {
            type Value = ChainsConfig;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map of chain identifiers to chain configurations")
            }

            fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut chains = Vec::with_capacity(access.size_hint().unwrap_or(0));

                while let Some(chain_id) = access.next_key::<ChainId>()? {
                    let namespace = chain_id.namespace();
                    let config = match namespace {
                        eip155::EIP155_NAMESPACE => {
                            let inner: Eip155ChainConfigInner = access.next_value()?;
                            let config = Eip155ChainConfig {
                                chain_reference: chain_id
                                    .try_into()
                                    .map_err(|e| serde::de::Error::custom(format!("{}", e)))?,
                                inner,
                            };
                            ChainConfig::Eip155(Box::new(config))
                        }
                        solana::SOLANA_NAMESPACE => {
                            let inner: SolanaChainConfigInner = access.next_value()?;
                            let config = SolanaChainConfig {
                                chain_reference: chain_id
                                    .try_into()
                                    .map_err(|e| serde::de::Error::custom(format!("{}", e)))?,
                                inner,
                            };
                            ChainConfig::Solana(Box::new(config))
                        }
                        aptos::APTOS_NAMESPACE => {
                            let inner: AptosChainConfigInner = access.next_value()?;
                            let config = AptosChainConfig {
                                chain_reference: chain_id
                                    .try_into()
                                    .map_err(|e| serde::de::Error::custom(format!("{}", e)))?,
                                inner,
                            };
                            ChainConfig::Aptos(Box::new(config))
                        }
                        _ => {
                            return Err(serde::de::Error::custom(format!(
                                "Unexpected namespace: {}",
                                namespace
                            )));
                        }
                    };
                    chains.push(config)
                }

                Ok(ChainsConfig(chains))
            }
        }

        deserializer.deserialize_map(ChainsVisitor)
    }
}

impl<TChainsConfig> Default for Config<TChainsConfig>
where
    TChainsConfig: Default,
{
    fn default() -> Self {
        Config {
            port: config_defaults::default_port(),
            host: config_defaults::default_host(),
            chains: TChainsConfig::default(),
            schemes: Vec::new(),
        }
    }
}

pub mod config_defaults {
    use std::env;
    use std::net::IpAddr;

    pub const DEFAULT_PORT: u16 = 8080;
    pub const DEFAULT_HOST: &str = "0.0.0.0";

    /// Returns the default port value with fallback: $PORT env var -> 8080
    pub fn default_port() -> u16 {
        env::var("PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_PORT)
    }

    /// Returns the default host value with fallback: $HOST env var -> "0.0.0.0"
    pub fn default_host() -> IpAddr {
        env::var("HOST")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(IpAddr::V4(DEFAULT_HOST.parse().unwrap()))
    }
}

/// Configuration error types.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("Failed to read config file at {0}: {1}")]
    FileRead(PathBuf, std::io::Error),
    #[error("Failed to parse config file: {0}")]
    JsonParse(#[from] serde_json::Error),
}

impl<TChainsConfig> Config<TChainsConfig> {
    /// Get the port value.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Get the host value as an IpAddr.
    ///
    /// Returns an error if the host string cannot be parsed as an IP address.
    pub fn host(&self) -> IpAddr {
        self.host
    }

    /// Get the schemes configuration list.
    ///
    /// Each entry specifies a scheme and the chains it applies to.
    pub fn schemes(&self) -> &Vec<SchemeConfig> {
        &self.schemes
    }

    /// Get the chains configuration map.
    ///
    /// Keys are CAIP-2 chain identifiers (e.g., "eip155:84532", "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp").
    pub fn chains(&self) -> &TChainsConfig {
        &self.chains
    }
}

impl<TChainsConfig> Config<TChainsConfig>
where
    TChainsConfig: Default + for<'de> Deserialize<'de>,
{
    /// Load configuration from CLI arguments and JSON file.
    ///
    /// The config file path is determined by:
    /// 1. `--config <path>` CLI argument
    /// 2. `./config.json` (if it exists)
    ///
    /// Values not present in the config file will be resolved via
    /// environment variables or defaults during deserialization.
    pub fn load() -> Result<Self, ConfigError> {
        let cli_args = CliArgs::parse();
        let config_path = Path::new(&cli_args.config)
            .canonicalize()
            .map_err(|e| ConfigError::FileRead(cli_args.config, e))?;
        Self::load_from_path(config_path)
    }

    /// Load configuration from a specific path (or use defaults if None).
    fn load_from_path(path: PathBuf) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(&path).map_err(|e| ConfigError::FileRead(path, e))?;
        let config: Config<TChainsConfig> = serde_json::from_str(&content)?;
        Ok(config)
    }
}
