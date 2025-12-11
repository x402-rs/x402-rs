//! Configuration module for the x402 facilitator server.

use alloy_primitives::B256;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::net::IpAddr;
use std::ops::Deref;
use std::path::PathBuf;
use std::str::FromStr;
use url::Url;

use crate::chain::chain_id::ChainId;
use crate::chain::evm::EvmChainReference;
use crate::chain::solana::SolanaChainReference;
use crate::types::MixedAddress;

/// CLI arguments for the x402 facilitator server.
#[derive(Parser, Debug)]
#[command(name = "x402-rs")]
#[command(about = "x402 Facilitator HTTP server")]
struct CliArgs {
    /// Path to the JSON configuration file
    #[arg(long = "config", short = 'c')]
    config: Option<PathBuf>,
}

/// Server configuration.
///
/// Fields use serde defaults that fall back to environment variables,
/// then to hardcoded defaults.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "config_defaults::default_port")]
    port: u16,
    #[serde(default = "config_defaults::default_host")]
    host: IpAddr,
    #[serde(default, with = "chains_serde")]
    chains: Vec<ChainConfig>,
}

/// Configuration for a specific chain.
///
/// This enum represents chain-specific configuration that varies by chain family
/// (EVM vs Solana). The chain family is determined by the CAIP-2 prefix of the
/// chain identifier key (e.g., "eip155:" for EVM, "solana:" for Solana).
#[derive(Debug, Clone)]
pub enum ChainConfig {
    /// EVM chain configuration (for chains with "eip155:" prefix).
    Evm(EvmChainConfig),
    /// Solana chain configuration (for chains with "solana:" prefix).
    Solana(SolanaChainConfig),
}

impl ChainConfig {
    pub fn chain_id(&self) -> ChainId {
        match self {
            ChainConfig::Evm(config) => config.chain_reference.into(),
            ChainConfig::Solana(config) => config.chain_reference.clone().into(),
        }
    }
}

/// EIP-712 domain configuration for token signatures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip712Config {
    /// The name field for EIP-712 domain (e.g., "USDC", "USD Coin").
    pub name: String,
    /// The version field for EIP-712 domain (e.g., "2").
    pub version: String,
}

/// USDC deployment configuration for a specific chain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct USDCConfig {
    /// The USDC contract address (EVM 0x-prefixed hex or Solana base58).
    pub address: MixedAddress,
    /// Number of decimals for the token (typically 6 for USDC).
    #[serde(default = "default_usdc_decimals")]
    pub decimals: u8,
    /// EIP-712 domain configuration (required for EVM chains, optional for Solana).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eip712: Option<Eip712Config>,
}

fn default_usdc_decimals() -> u8 {
    6
}

/// RPC provider configuration for a single provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcProviderConfig {
    /// HTTP URL for the RPC endpoint.
    pub http: Url,
    /// Rate limit for requests per second (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<u32>,
}

/// RPC configuration containing multiple named providers.
///
/// Uses serde flatten to allow a map of provider names to their configurations:
/// ```json
/// {
///   "quicknode": { "http": "https://...", "rate_limit": 50 },
///   "alchemy": { "http": "https://...", "rate_limit": 100 }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RpcConfig {
    /// Map of provider name to provider configuration.
    #[serde(flatten)]
    pub providers: BTreeMap<String, RpcProviderConfig>,
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
#[derive(Debug, Clone)]
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

impl<T> std::ops::Deref for LiteralOrEnv<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> std::ops::DerefMut for LiteralOrEnv<T> {
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

impl<T: PartialEq> PartialEq for LiteralOrEnv<T> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0
    }
}

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
    /// Parse a hex string (with or without 0x prefix) into a private key.
    pub fn from_hex(s: &str) -> Result<Self, String> {
        let hex_str = s.strip_prefix("0x").unwrap_or(s);

        if hex_str.len() != 64 {
            return Err(format!(
                "Private key must be 32 bytes (64 hex chars), got {} chars",
                hex_str.len()
            ));
        }

        let bytes = B256::from_str(s).map_err(|e| format!("Invalid hex: {}", e))?;
        Ok(Self(bytes))
    }

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
        Self::from_hex(s)
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
pub type EvmSignersConfig = Vec<LiteralOrEnv<EvmPrivateKey>>;

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

    /// Get the raw 64 bytes of the keypair.
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.0
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
pub struct EvmChainConfig {
    chain_reference: EvmChainReference,
    inner: EvmChainConfigInner,
}

impl EvmChainConfig {
    pub fn eip1559(&self) -> bool {
        self.inner.eip1559
    }
    pub fn flashblocks(&self) -> bool {
        self.inner.flashblocks
    }
    pub fn usdc(&self) -> Option<&USDCConfig> {
        self.inner.usdc.as_ref()
    }
    pub fn signers(&self) -> &EvmSignersConfig {
        &self.inner.signers
    }
    pub fn rpc(&self) -> &RpcConfig {
        &self.inner.rpc
    }
}

#[derive(Debug, Clone)]
pub struct SolanaChainConfig {
    chain_reference: SolanaChainReference,
    inner: SolanaChainConfigInner,
}

impl SolanaChainConfig {
    pub fn usdc(&self) -> Option<&USDCConfig> {
        self.inner.usdc.as_ref()
    }
    pub fn signer(&self) -> &SolanaSignerConfig {
        &self.inner.signer
    }
    pub fn rpc(&self) -> &RpcConfig {
        &self.inner.rpc
    }
}

/// Configuration specific to EVM-compatible chains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvmChainConfigInner {
    /// Whether the chain supports EIP-1559 gas pricing.
    #[serde(default)]
    pub eip1559: bool,
    /// Whether the chain supports flashblocks.
    #[serde(default)]
    pub flashblocks: bool,
    /// USDC deployment configuration for this chain (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usdc: Option<USDCConfig>,
    /// Signer configuration for this chain (required).
    /// Array of private keys (hex format) or env var references.
    pub signers: EvmSignersConfig,
    /// RPC provider configuration for this chain (required).
    pub rpc: RpcConfig,
}

/// Configuration specific to Solana chains.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaChainConfigInner {
    /// USDC deployment configuration for this chain (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usdc: Option<USDCConfig>,
    /// Signer configuration for this chain (required).
    /// A single private key (base58 format, 64 bytes) or env var reference.
    pub signer: SolanaSignerConfig,
    /// RPC provider configuration for this chain (required).
    pub rpc: RpcConfig,
}

/// Custom serde module for deserializing the chains map with type discrimination
/// based on the CAIP-2 chain identifier prefix.
mod chains_serde {
    use super::{
        ChainConfig, ChainId, EvmChainConfig, EvmChainConfigInner, SolanaChainConfig,
        SolanaChainConfigInner,
    };
    use crate::chain::Namespace;
    use serde::de::{MapAccess, Visitor};
    use serde::ser::SerializeMap;
    use serde::{Deserializer, Serializer};
    use std::fmt;
    use std::str::FromStr;

    #[allow(dead_code)]
    pub fn serialize<S>(chains: &Vec<ChainConfig>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(chains.len()))?;
        for chain_config in chains {
            match chain_config {
                ChainConfig::Evm(config) => {
                    let chain_id: ChainId = config.chain_reference.clone().into();
                    let inner = &config.inner;
                    map.serialize_entry(&chain_id, inner)?;
                }
                ChainConfig::Solana(config) => {
                    let chain_id: ChainId = config.chain_reference.clone().into();
                    let inner = &config.inner;
                    map.serialize_entry(&chain_id, inner)?;
                }
            }
        }
        map.end()
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<ChainConfig>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ChainsVisitor;

        impl<'de> Visitor<'de> for ChainsVisitor {
            type Value = Vec<ChainConfig>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map of chain identifiers to chain configurations")
            }

            fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut chains = Vec::with_capacity(access.size_hint().unwrap_or(0));

                while let Some(chain_id) = access.next_key::<ChainId>()? {
                    let namespace = Namespace::from_str(&chain_id.namespace)
                        .map_err(|e| serde::de::Error::custom(format!("{}", e)))?;
                    let config = match namespace {
                        Namespace::Eip155 => {
                            let inner: EvmChainConfigInner = access.next_value()?;
                            let config = EvmChainConfig {
                                chain_reference: chain_id
                                    .try_into()
                                    .map_err(|e| serde::de::Error::custom(format!("{}", e)))?,
                                inner,
                            };
                            ChainConfig::Evm(config)
                        }
                        Namespace::Solana => {
                            let inner: SolanaChainConfigInner = access.next_value()?;
                            let config = SolanaChainConfig {
                                chain_reference: chain_id
                                    .try_into()
                                    .map_err(|e| serde::de::Error::custom(format!("{}", e)))?,
                                inner,
                            };
                            ChainConfig::Solana(config)
                        }
                    };

                    chains.push(config)
                }

                Ok(chains)
            }
        }

        deserializer.deserialize_map(ChainsVisitor)
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            port: config_defaults::default_port(),
            host: config_defaults::default_host(),
            chains: Vec::new(),
        }
    }
}

mod config_defaults {
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
    #[error("Failed to read config file: {0}")]
    FileRead(#[from] std::io::Error),
    #[error("Failed to parse config file: {0}")]
    JsonParse(#[from] serde_json::Error),
}

impl Config {
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
        let config_path = Self::get_config_path(cli_args.config);
        Self::load_from_path(config_path)
    }

    /// Load configuration from a specific path (or use defaults if None).
    fn load_from_path(path: Option<PathBuf>) -> Result<Self, ConfigError> {
        match path {
            Some(p) => {
                let content = fs::read_to_string(&p)?;
                let config: Config = serde_json::from_str(&content)?;
                Ok(config)
            }
            None => Ok(Config::default()),
        }
    }

    /// Get the config file path from CLI arguments or default to `./config.json`.
    fn get_config_path(cli_config: Option<PathBuf>) -> Option<PathBuf> {
        // If --config was provided via CLI, use it
        if let Some(path) = cli_config {
            return Some(path);
        }

        // Default to ./config.json if it exists
        let default_path = PathBuf::from("config.json");
        if default_path.exists() {
            Some(default_path)
        } else {
            None
        }
    }

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

    /// Get the chains configuration map.
    ///
    /// Keys are CAIP-2 chain identifiers (e.g., "eip155:84532", "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp").
    pub fn chains(&self) -> &Vec<ChainConfig> {
        &self.chains
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::str::FromStr;
    use std::sync::Mutex;

    // Mutex to prevent concurrent env var modifications in tests
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    // Test private keys (DO NOT use in production!)
    const TEST_EVM_KEY_1: &str =
        "0xcafe000000000000000000000000000000000000000000000000000000000001";
    const TEST_EVM_KEY_2: &str =
        "0xcafe000000000000000000000000000000000000000000000000000000000002";
    // A valid 64-byte Solana keypair in base58 format (test key, DO NOT use in production!)
    // Standard Solana format: 32-byte secret key + 32-byte public key
    // Generated with `solana_keypair::Keypair::new()`
    const TEST_SOLANA_KEY: &str =
        "D2hT1mgysgZYJ8ZaS93m5sgZSRUPntaUdMoN7b5potSnidpfTd2nsRhzyi333BhY7PvGBtMiUHkXL3gTNDsdCYK";

    #[test]
    fn test_config_parsing_full() {
        let json = r#"{"port": 3000, "host": "127.0.0.1"}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.port(), 3000);
        assert_eq!(config.host().to_string(), "127.0.0.1");
    }

    #[test]
    fn test_config_parsing_partial_port_only() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        // Clear env vars for predictable test
        // SAFETY: This is safe in a single-threaded test context
        unsafe { env::remove_var("HOST") };

        let json = r#"{"port": 3000}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.port(), 3000);
        assert_eq!(config.host().to_string(), "0.0.0.0");
    }

    #[test]
    fn test_config_parsing_partial_host_only() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        // Clear env vars for predictable test
        // SAFETY: This is safe in a single-threaded test context
        unsafe { env::remove_var("PORT") };

        let json = r#"{"host": "127.0.0.1"}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.port(), 8080);
        assert_eq!(config.host().to_string(), "127.0.0.1");
    }

    #[test]
    fn test_config_parsing_empty() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        // Clear env vars for predictable test
        // SAFETY: This is safe in a single-threaded test context
        unsafe {
            env::remove_var("PORT");
            env::remove_var("HOST");
        }

        let json = r#"{}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.port(), 8080);
        assert_eq!(config.host().to_string(), "0.0.0.0");
    }

    #[test]
    fn test_config_default() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        // Clear env vars for predictable test
        // SAFETY: This is safe in a single-threaded test context
        unsafe {
            env::remove_var("PORT");
            env::remove_var("HOST");
        }

        let config = Config::default();
        assert_eq!(config.port(), 8080);
        assert_eq!(config.host().to_string(), "0.0.0.0");
    }

    #[test]
    fn test_get_config_path_with_cli_arg() {
        let path = Config::get_config_path(Some(PathBuf::from("/custom/config.json")));
        assert_eq!(path, Some(PathBuf::from("/custom/config.json")));
    }

    #[test]
    fn test_get_config_path_without_cli_arg_no_default() {
        // When no CLI arg and no config.json exists, should return None
        // This test assumes config.json doesn't exist in the test directory
        let path = Config::get_config_path(None);
        // Result depends on whether config.json exists in cwd
        // We just verify it doesn't panic
        let _ = path;
    }

    #[test]
    fn test_config_parsing_with_chains_and_signers() {
        let json = format!(
            r#"{{
            "port": 3000,
            "host": "127.0.0.1",
            "chains": {{
                "eip155:84532": {{
                    "v1_name": "base-sepolia",
                    "eip1559": true,
                    "flashblocks": true,
                    "usdc": {{
                        "address": "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
                        "decimals": 6,
                        "eip712": {{
                            "name": "USDC",
                            "version": "2"
                        }}
                    }},
                    "signers": ["{}"],
                    "rpc": {{
                        "default": {{
                            "http": "https://example.quiknode.pro/"
                        }}
                    }}
                }},
                "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp": {{
                    "v1_name": "solana",
                    "usdc": {{
                        "address": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                        "decimals": 6
                    }},
                    "signer": "{}",
                    "rpc": {{
                        "default": {{
                            "http": "https://mainnet.helius-rpc.com/?api-key=key"
                        }}
                    }}
                }}
            }}
        }}"#,
            TEST_EVM_KEY_1, TEST_SOLANA_KEY
        );
        let config: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(config.port(), 3000);
        assert_eq!(config.host().to_string(), "127.0.0.1");
        assert_eq!(config.chains().len(), 2);

        // Verify EVM chain config
        let evm_key = ChainId::from_str("eip155:84532").unwrap();
        let evm_config = config.chains().iter().find(|c| c.chain_id().eq(&evm_key)).unwrap();
        match evm_config {
            ChainConfig::Evm(evm) => {
                assert!(evm.eip1559());
                assert!(evm.flashblocks());
                assert!(evm.usdc().is_some());
                let usdc = evm.usdc().unwrap();
                assert_eq!(usdc.decimals, 6);
                assert!(usdc.eip712.is_some());
                let eip712 = usdc.eip712.as_ref().unwrap();
                assert_eq!(eip712.name, "USDC");
                assert_eq!(eip712.version, "2");
                assert!(!evm.rpc().providers.is_empty());
                // Verify signers
                assert_eq!(evm.signers().len(), 1);
            }
            _ => panic!("Expected EVM config"),
        }

        // Verify Solana chain config
        let solana_key = ChainId::from_str("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp").unwrap();
        let solana_config = config.chains().iter().find(|c| c.chain_id().eq(&solana_key)).unwrap();
        match solana_config {
            ChainConfig::Solana(solana) => {
                assert!(solana.usdc().is_some());
                let usdc = solana.usdc().unwrap();
                assert_eq!(usdc.decimals, 6);
                assert!(usdc.eip712.is_none());
                assert!(!solana.rpc().providers.is_empty());
                // Verify signer is present (using Deref)
                let _: &SolanaPrivateKey = &solana.signer().0;
            }
            _ => panic!("Expected Solana config"),
        }
    }

    #[test]
    fn test_config_parsing_evm_defaults_with_signers() {
        let json = format!(
            r#"{{
            "chains": {{
                "eip155:8453": {{
                    "v1_name": "base",
                    "usdc": {{
                        "address": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
                        "eip712": {{
                            "name": "USD Coin",
                            "version": "2"
                        }}
                    }},
                    "signers": ["{}"],
                    "rpc": {{
                        "default": {{
                            "http": "https://base-rpc.example.com/"
                        }}
                    }}
                }}
            }}
        }}"#,
            TEST_EVM_KEY_1
        );
        let config: Config = serde_json::from_str(&json).unwrap();
        let evm_key = ChainId::from_str("eip155:8453").unwrap();
        let evm_config = config.chains().iter().find(|c| c.chain_id().eq(&evm_key)).unwrap();
        match evm_config {
            ChainConfig::Evm(evm) => {
                assert!(!evm.eip1559()); // default false
                assert!(!evm.flashblocks()); // default false
                assert!(evm.usdc().is_some());
                assert_eq!(evm.usdc().unwrap().decimals, 6); // default 6
            }
            _ => panic!("Expected EVM config"),
        }
    }

    #[test]
    fn test_config_parsing_without_usdc_with_signers() {
        let json = format!(
            r#"{{
            "chains": {{
                "eip155:8453": {{
                    "v1_name": "base",
                    "signers": ["{}"],
                    "rpc": {{
                        "default": {{
                            "http": "https://base-rpc.example.com/"
                        }}
                    }}
                }},
                "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp": {{
                    "v1_name": "solana",
                    "signer": "{}",
                    "rpc": {{
                        "default": {{
                            "http": "https://solana-rpc.example.com/"
                        }}
                    }}
                }}
            }}
        }}"#,
            TEST_EVM_KEY_1, TEST_SOLANA_KEY
        );
        let config: Config = serde_json::from_str(&json).unwrap();

        // Verify EVM chain config without usdc
        let evm_key = ChainId::from_str("eip155:8453").unwrap();
        let evm_config = config.chains().iter().find(|c| c.chain_id().eq(&evm_key)).unwrap();
        match evm_config {
            ChainConfig::Evm(evm) => {
                assert!(evm.usdc().is_none());
            }
            _ => panic!("Expected EVM config"),
        }

        // Verify Solana chain config without usdc
        let solana_key = ChainId::from_str("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp").unwrap();
        let solana_config = config.chains().iter().find(|c| c.chain_id().eq(&solana_key)).unwrap();
        match solana_config {
            ChainConfig::Solana(solana) => {
                assert!(solana.usdc().is_none());
            }
            _ => panic!("Expected Solana config"),
        }
    }

    #[test]
    fn test_config_parsing_unknown_chain_family() {
        let json = r#"{
            "chains": {
                "unknown:12345": {
                    "v1_name": "test"
                }
            }
        }"#;
        let result: Result<Config, _> = serde_json::from_str(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("unsupported namespace"));
    }

    #[test]
    fn test_config_parsing_empty_chains() {
        let json = r#"{"chains": {}}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.chains().is_empty());
    }

    #[test]
    fn test_config_parsing_no_chains_field() {
        let json = r#"{"port": 8080}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert!(config.chains().is_empty());
    }

    #[test]
    fn test_config_parsing_with_rpc_and_signers() {
        let json = format!(
            r#"{{
            "chains": {{
                "eip155:84532": {{
                    "v1_name": "base-sepolia",
                    "signers": ["{}"],
                    "rpc": {{
                        "quicknode": {{
                            "http": "https://example.quiknode.pro/",
                            "rate_limit": 50
                        }},
                        "alchemy": {{
                            "http": "https://base-sepolia.g.alchemy.com/v2/key"
                        }}
                    }}
                }},
                "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp": {{
                    "v1_name": "solana",
                    "signer": "{}",
                    "rpc": {{
                        "helius": {{
                            "http": "https://mainnet.helius-rpc.com/?api-key=key",
                            "rate_limit": 100
                        }}
                    }}
                }}
            }}
        }}"#,
            TEST_EVM_KEY_1, TEST_SOLANA_KEY
        );
        let config: Config = serde_json::from_str(&json).unwrap();
        assert_eq!(config.chains().len(), 2);

        // Verify EVM chain config with RPC
        let evm_key = ChainId::from_str("eip155:84532").unwrap();
        let evm_config = config.chains().iter().find(|c| c.chain_id().eq(&evm_key)).unwrap();
        match evm_config {
            ChainConfig::Evm(evm) => {
                assert_eq!(evm.rpc().providers.len(), 2);

                let quicknode = evm.rpc().providers.get("quicknode").unwrap();
                assert_eq!(quicknode.http.as_str(), "https://example.quiknode.pro/");
                assert_eq!(quicknode.rate_limit, Some(50));

                let alchemy = evm.rpc().providers.get("alchemy").unwrap();
                assert_eq!(
                    alchemy.http.as_str(),
                    "https://base-sepolia.g.alchemy.com/v2/key"
                );
                assert!(alchemy.rate_limit.is_none());
            }
            _ => panic!("Expected EVM config"),
        }

        // Verify Solana chain config with RPC
        let solana_key = ChainId::from_str("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp").unwrap();
        let solana_config = config.chains().iter().find(|c| c.chain_id().eq(&solana_key)).unwrap();
        match solana_config {
            ChainConfig::Solana(solana) => {
                assert_eq!(solana.rpc().providers.len(), 1);

                let helius = solana.rpc().providers.get("helius").unwrap();
                assert_eq!(
                    helius.http.as_str(),
                    "https://mainnet.helius-rpc.com/?api-key=key"
                );
                assert_eq!(helius.rate_limit, Some(100));
            }
            _ => panic!("Expected Solana config"),
        }
    }

    #[test]
    fn test_config_parsing_missing_signers_fails() {
        let json = r#"{
            "chains": {
                "eip155:8453": {
                    "v1_name": "base",
                    "rpc": {
                        "default": {
                            "http": "https://base-rpc.example.com/"
                        }
                    }
                }
            }
        }"#;
        let result: Result<Config, _> = serde_json::from_str(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("signers") || err.contains("missing field"));
    }

    #[test]
    fn test_config_parsing_missing_rpc_fails() {
        let json = format!(
            r#"{{
            "chains": {{
                "eip155:8453": {{
                    "v1_name": "base",
                    "signers": ["{}"]
                }}
            }}
        }}"#,
            TEST_EVM_KEY_1
        );
        let result: Result<Config, _> = serde_json::from_str(&json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("rpc") || err.contains("missing field"));
    }

    // ========================================================================
    // EVM Signers Config Tests
    // ========================================================================

    #[test]
    fn test_evm_signers_single_key() {
        let json = format!(r#"["{}"]"#, TEST_EVM_KEY_1);
        let signers: EvmSignersConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(signers.len(), 1);
    }

    #[test]
    fn test_evm_signers_multiple_keys() {
        let json = format!(r#"["{}", "{}"]"#, TEST_EVM_KEY_1, TEST_EVM_KEY_2);
        let signers: EvmSignersConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(signers.len(), 2);
    }

    #[test]
    fn test_evm_signers_from_env_var() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe { env::set_var("TEST_EVM_KEY", TEST_EVM_KEY_1) };

        let json = r#"["$TEST_EVM_KEY"]"#;
        let signers: EvmSignersConfig = serde_json::from_str(json).unwrap();
        assert_eq!(signers.len(), 1);

        unsafe { env::remove_var("TEST_EVM_KEY") };
    }

    #[test]
    fn test_evm_signers_from_braced_env_var() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe { env::set_var("TEST_EVM_KEY_BRACED", TEST_EVM_KEY_1) };

        let json = r#"["${TEST_EVM_KEY_BRACED}"]"#;
        let signers: EvmSignersConfig = serde_json::from_str(json).unwrap();
        assert_eq!(signers.len(), 1);

        unsafe { env::remove_var("TEST_EVM_KEY_BRACED") };
    }

    #[test]
    fn test_evm_signers_mixed_literal_and_env() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe { env::set_var("TEST_EVM_KEY_MIXED", TEST_EVM_KEY_2) };

        let json = format!(r#"["{}", "$TEST_EVM_KEY_MIXED"]"#, TEST_EVM_KEY_1);
        let signers: EvmSignersConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(signers.len(), 2);

        unsafe { env::remove_var("TEST_EVM_KEY_MIXED") };
    }

    #[test]
    fn test_evm_signers_missing_env_var_fails() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe { env::remove_var("NONEXISTENT_EVM_KEY") };

        let json = r#"["$NONEXISTENT_EVM_KEY"]"#;
        let result: Result<EvmSignersConfig, _> = serde_json::from_str(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("NONEXISTENT_EVM_KEY"));
    }

    #[test]
    fn test_evm_signers_invalid_key_fails() {
        let json = r#"["not-a-valid-hex-key"]"#;
        let result: Result<EvmSignersConfig, _> = serde_json::from_str(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Failed to parse value"));
    }

    // ========================================================================
    // Solana Signer Config Tests
    // ========================================================================

    #[test]
    fn test_solana_signer_single_key() {
        let json = format!(r#""{}""#, TEST_SOLANA_KEY);
        let signer: SolanaSignerConfig = serde_json::from_str(&json).unwrap();
        // Verify we can access the key via Deref
        let _: &SolanaPrivateKey = &signer;
    }

    #[test]
    fn test_solana_signer_from_env_var() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe { env::set_var("TEST_SOLANA_KEY", TEST_SOLANA_KEY) };

        let json = r#""$TEST_SOLANA_KEY""#;
        let signer: SolanaSignerConfig = serde_json::from_str(json).unwrap();
        let _: &SolanaPrivateKey = &signer;

        unsafe { env::remove_var("TEST_SOLANA_KEY") };
    }

    #[test]
    fn test_solana_signer_from_braced_env_var() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe { env::set_var("TEST_SOLANA_KEY_BRACED", TEST_SOLANA_KEY) };

        let json = r#""${TEST_SOLANA_KEY_BRACED}""#;
        let signer: SolanaSignerConfig = serde_json::from_str(json).unwrap();
        let _: &SolanaPrivateKey = &signer;

        unsafe { env::remove_var("TEST_SOLANA_KEY_BRACED") };
    }

    #[test]
    fn test_solana_signer_missing_env_var_fails() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        unsafe { env::remove_var("NONEXISTENT_SOLANA_KEY") };

        let json = r#""$NONEXISTENT_SOLANA_KEY""#;
        let result: Result<SolanaSignerConfig, _> = serde_json::from_str(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("NONEXISTENT_SOLANA_KEY"));
    }

    #[test]
    fn test_solana_signer_invalid_key_fails() {
        let json = r#""not-a-valid-base58-key""#;
        let result: Result<SolanaSignerConfig, _> = serde_json::from_str(json);
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Failed to parse"));
    }
}
