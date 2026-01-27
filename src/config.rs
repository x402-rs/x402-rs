//! Configuration module for the x402 facilitator server.

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::IpAddr;
use std::ops::Deref;
use std::path::{Path, PathBuf};
use x402_chain_eip155::chain as eip155;
use x402_chain_eip155::chain::config::{Eip155ChainConfig, Eip155ChainConfigInner};
use x402_chain_solana::chain as solana;
use x402_chain_solana::chain::config::{SolanaChainConfig, SolanaChainConfigInner};
use x402_types::chain::ChainId;
use x402_types::scheme::SchemeConfig;

#[cfg(feature = "aptos")]
use crate::chain::aptos;

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
    #[cfg(feature = "aptos")]
    Aptos(Box<AptosChainConfig>),
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
#[cfg(feature = "aptos")]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AptosPrivateKey(Vec<u8>);

#[cfg(feature = "aptos")]
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

#[cfg(feature = "aptos")]
impl Serialize for AptosPrivateKey {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

#[cfg(feature = "aptos")]
impl FromStr for AptosPrivateKey {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::from_hex(s)
    }
}

#[cfg(feature = "aptos")]
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
#[cfg(feature = "aptos")]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AptosSignerConfig(LiteralOrEnv<AptosPrivateKey>);

#[cfg(feature = "aptos")]
impl Deref for AptosSignerConfig {
    type Target = AptosPrivateKey;

    fn deref(&self) -> &Self::Target {
        self.0.inner()
    }
}

// ============================================================================
// Aptos Chain Configuration
// ============================================================================

#[cfg(feature = "aptos")]
#[derive(Debug, Clone)]
pub struct AptosChainConfig {
    pub chain_reference: aptos::AptosChainReference,
    pub inner: AptosChainConfigInner,
}

#[cfg(feature = "aptos")]
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
    pub fn chain_reference(&self) -> aptos::AptosChainReference {
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
#[cfg(feature = "aptos")]
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

#[cfg(feature = "aptos")]
mod aptos_chain_config {
    use super::LiteralOrEnv;

    pub fn default_sponsor_gas() -> LiteralOrEnv<bool> {
        // Default to false when field is missing
        LiteralOrEnv::from_literal(false)
    }
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
                #[cfg(feature = "aptos")]
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
                        #[cfg(feature = "aptos")]
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
