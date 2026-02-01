//! Configuration types for x402 infrastructure.
//!
//! This module provides the core configuration types used throughout the x402 ecosystem,
//! including server configuration, RPC provider configuration, CLI argument parsing,
//! and environment variable resolution.
//!
//! # Overview
//!
//! The configuration system is designed to be reusable across different x402 components:
//!
//! - [`Config<T>`] - Generic server configuration parameterized by chain config type
//! - [`CliArgs`] - CLI argument parsing (requires `cli` feature)
//! - [`LiteralOrEnv`] - Transparent wrapper for environment variable resolution
//!
//! # Configuration File Format
//!
//! Configuration is loaded from a JSON file (default: `config.json`) with the following structure:
//!
//! ```json
//! {
//!   "port": 8080,
//!   "host": "0.0.0.0",
//!   "chains": { /* chain-specific configuration */ },
//!   "schemes": [
//!     { "scheme": "v2-eip155-exact", "chains": ["eip155:8453"] }
//!   ]
//! }
//! ```
//!
//! # Environment Variables
//!
//! - `CONFIG` - Path to configuration file (default: `config.json`)
//! - `PORT` - Server port (default: 8080)
//! - `HOST` - Server bind address (default: `0.0.0.0`)
//!
//! # Environment Variable Resolution
//!
//! The [`LiteralOrEnv`] wrapper type allows configuration values to be specified
//! either as literal values or as references to environment variables:
//!
//! ```json
//! {
//!   "http": "http://localhost:8545",           // Literal value
//!   "api_key": "$API_KEY",                     // Simple env var
//!   "secret": "${DATABASE_SECRET}"             // Braced env var
//! }
//! ```
//!
//! This is particularly useful for keeping secrets out of configuration files
//! while still allowing them to be loaded at runtime.
//!
//! # Feature Flags
//!
//! - `cli` - Enables CLI argument parsing via [`clap`]. When enabled, [`Config::load()`]
//!   parses command-line arguments to determine the config file path.

use serde::{Deserialize, Serialize};
use std::fs;
use std::net::IpAddr;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::str::FromStr;

#[cfg(feature = "cli")]
use clap::Parser;
#[cfg(feature = "cli")]
use std::path::Path;

use crate::scheme::SchemeConfig;

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
    pub fn from_literal(value: T) -> Self {
        Self(value)
    }

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

/// CLI arguments for the x402 facilitator server.
#[derive(Debug)]
#[cfg_attr(feature = "cli", derive(Parser))]
#[cfg_attr(feature = "cli", command(name = "x402-rs"))]
#[cfg_attr(feature = "cli", command(about = "x402 Facilitator HTTP server"))]
#[allow(dead_code)] // For downstream crates to use
pub struct CliArgs {
    /// Path to the JSON configuration file
    #[cfg_attr(
        feature = "cli",
        arg(long, short, env = "CONFIG", default_value = "config.json")
    )]
    pub config: PathBuf,
}

/// Server configuration.
///
/// Fields use serde defaults that fall back to environment variables,
/// then to hardcoded defaults.
#[derive(Debug, Clone, Deserialize)]
pub struct Config<TChainsConfig> {
    #[serde(default = "config_defaults::default_port")]
    port: u16,
    #[serde(default = "config_defaults::default_host")]
    host: IpAddr,
    #[serde(default)]
    chains: TChainsConfig,
    #[serde(default)]
    schemes: Vec<SchemeConfig>,
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
    #[cfg(feature = "cli")]
    pub fn load() -> Result<Self, ConfigError> {
        let cli_args = CliArgs::parse();
        let config_path = Path::new(&cli_args.config)
            .canonicalize()
            .map_err(|e| ConfigError::FileRead(cli_args.config, e))?;
        Self::load_from_path(config_path)
    }

    /// Load configuration from a specific path (or use defaults if None).
    pub fn load_from_path(path: PathBuf) -> Result<Self, ConfigError> {
        let content = fs::read_to_string(&path).map_err(|e| ConfigError::FileRead(path, e))?;
        let config: Config<TChainsConfig> = serde_json::from_str(&content)?;
        Ok(config)
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
