//! Configuration module for the x402 facilitator server.

use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::net::IpAddr;
use std::path::PathBuf;

use crate::chain::chain_id::ChainId;
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
    chains: ChainsConfigMap,
}

/// A mapping from CAIP-2 chain identifiers to their respective chain configurations.
pub type ChainsConfigMap = HashMap<ChainId, ChainConfig>;

/// Configuration for a specific chain.
///
/// This enum represents chain-specific configuration that varies by chain family
/// (EVM vs Solana). The chain family is determined by the CAIP-2 prefix of the
/// chain identifier key (e.g., "eip155:" for EVM, "solana:" for Solana).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ChainConfig {
    /// EVM chain configuration (for chains with "eip155:" prefix).
    Evm(EvmChainConfig),
    /// Solana chain configuration (for chains with "solana:" prefix).
    Solana(SolanaChainConfig),
}

/// EIP-712 domain configuration for token signatures.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Eip712Config {
    /// The name field for EIP-712 domain (e.g., "USDC", "USD Coin").
    pub name: String,
    /// The version field for EIP-712 domain (e.g., "2").
    pub version: String,
}

/// USDC deployment configuration for a specific chain.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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

/// Configuration specific to EVM-compatible chains.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EvmChainConfig {
    /// The v1 protocol name for this chain (e.g., "base-sepolia").
    pub v1_name: String,
    /// Whether the chain supports EIP-1559 gas pricing.
    #[serde(default)]
    pub eip1559: bool,
    /// Whether the chain supports flashblocks.
    #[serde(default)]
    pub flashblocks: bool,
    /// USDC deployment configuration for this chain (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usdc: Option<USDCConfig>,
}

/// Configuration specific to Solana chains.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SolanaChainConfig {
    /// The v1 protocol name for this chain (e.g., "solana").
    pub v1_name: String,
    /// USDC deployment configuration for this chain (optional).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usdc: Option<USDCConfig>,
}

/// Custom serde module for deserializing the chains map with type discrimination
/// based on the CAIP-2 chain identifier prefix.
mod chains_serde {
    use super::{ChainConfig, ChainId, EvmChainConfig, SolanaChainConfig};
    use crate::chain::chain_id::Namespace;
    use serde::de::{MapAccess, Visitor};
    use serde::ser::SerializeMap;
    use serde::{Deserializer, Serializer};
    use std::collections::HashMap;
    use std::fmt;

    #[allow(dead_code)]
    pub fn serialize<S>(
        chains: &HashMap<ChainId, ChainConfig>,
        serializer: S,
    ) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(chains.len()))?;
        for (key, value) in chains {
            map.serialize_entry(key, value)?;
        }
        map.end()
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<HashMap<ChainId, ChainConfig>, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ChainsVisitor;

        impl<'de> Visitor<'de> for ChainsVisitor {
            type Value = HashMap<ChainId, ChainConfig>;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map of chain identifiers to chain configurations")
            }

            fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                let mut map = HashMap::with_capacity(access.size_hint().unwrap_or(0));

                while let Some(key) = access.next_key::<ChainId>()? {
                    let config = match key.namespace {
                        Namespace::Eip155 => {
                            let evm_config: EvmChainConfig = access.next_value()?;
                            ChainConfig::Evm(evm_config)
                        }
                        Namespace::Solana => {
                            let solana_config: SolanaChainConfig = access.next_value()?;
                            ChainConfig::Solana(solana_config)
                        }
                    };

                    map.insert(key, config);
                }

                Ok(map)
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
            chains: HashMap::new(),
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
    pub fn chains(&self) -> &HashMap<ChainId, ChainConfig> {
        &self.chains
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use std::str::FromStr;

    #[test]
    fn test_config_parsing_full() {
        let json = r#"{"port": 3000, "host": "127.0.0.1"}"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.port(), 3000);
        assert_eq!(config.host().to_string(), "127.0.0.1");
    }

    #[test]
    fn test_config_parsing_partial_port_only() {
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
    fn test_config_parsing_with_chains() {
        let json = r#"{
            "port": 3000,
            "host": "127.0.0.1",
            "chains": {
                "eip155:84532": {
                    "v1_name": "base-sepolia",
                    "eip1559": true,
                    "flashblocks": true,
                    "usdc": {
                        "address": "0x036CbD53842c5426634e7929541eC2318f3dCF7e",
                        "decimals": 6,
                        "eip712": {
                            "name": "USDC",
                            "version": "2"
                        }
                    }
                },
                "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp": {
                    "v1_name": "solana",
                    "usdc": {
                        "address": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
                        "decimals": 6
                    }
                }
            }
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.port(), 3000);
        assert_eq!(config.host().to_string(), "127.0.0.1");
        assert_eq!(config.chains().len(), 2);

        // Verify EVM chain config
        let evm_key = ChainId::from_str("eip155:84532").unwrap();
        let evm_config = config.chains().get(&evm_key).unwrap();
        match evm_config {
            ChainConfig::Evm(evm) => {
                assert_eq!(evm.v1_name, "base-sepolia");
                assert!(evm.eip1559);
                assert!(evm.flashblocks);
                assert!(evm.usdc.is_some());
                let usdc = evm.usdc.as_ref().unwrap();
                assert_eq!(usdc.decimals, 6);
                assert!(usdc.eip712.is_some());
                let eip712 = usdc.eip712.as_ref().unwrap();
                assert_eq!(eip712.name, "USDC");
                assert_eq!(eip712.version, "2");
            }
            _ => panic!("Expected EVM config"),
        }

        // Verify Solana chain config
        let solana_key = ChainId::from_str("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp").unwrap();
        let solana_config = config.chains().get(&solana_key).unwrap();
        match solana_config {
            ChainConfig::Solana(solana) => {
                assert_eq!(solana.v1_name, "solana");
                assert!(solana.usdc.is_some());
                let usdc = solana.usdc.as_ref().unwrap();
                assert_eq!(usdc.decimals, 6);
                assert!(usdc.eip712.is_none());
            }
            _ => panic!("Expected Solana config"),
        }
    }

    #[test]
    fn test_config_parsing_evm_defaults() {
        let json = r#"{
            "chains": {
                "eip155:8453": {
                    "v1_name": "base",
                    "usdc": {
                        "address": "0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913",
                        "eip712": {
                            "name": "USD Coin",
                            "version": "2"
                        }
                    }
                }
            }
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        let evm_key = ChainId::from_str("eip155:8453").unwrap();
        let evm_config = config.chains().get(&evm_key).unwrap();
        match evm_config {
            ChainConfig::Evm(evm) => {
                assert_eq!(evm.v1_name, "base");
                assert!(!evm.eip1559); // default false
                assert!(!evm.flashblocks); // default false
                assert!(evm.usdc.is_some());
                assert_eq!(evm.usdc.as_ref().unwrap().decimals, 6); // default 6
            }
            _ => panic!("Expected EVM config"),
        }
    }

    #[test]
    fn test_config_parsing_without_usdc() {
        let json = r#"{
            "chains": {
                "eip155:8453": {
                    "v1_name": "base"
                },
                "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp": {
                    "v1_name": "solana"
                }
            }
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        
        // Verify EVM chain config without usdc
        let evm_key = ChainId::from_str("eip155:8453").unwrap();
        let evm_config = config.chains().get(&evm_key).unwrap();
        match evm_config {
            ChainConfig::Evm(evm) => {
                assert_eq!(evm.v1_name, "base");
                assert!(evm.usdc.is_none());
            }
            _ => panic!("Expected EVM config"),
        }

        // Verify Solana chain config without usdc
        let solana_key = ChainId::from_str("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp").unwrap();
        let solana_config = config.chains().get(&solana_key).unwrap();
        match solana_config {
            ChainConfig::Solana(solana) => {
                assert_eq!(solana.v1_name, "solana");
                assert!(solana.usdc.is_none());
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
        assert!(err.contains("unknown namespace"));
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
}
