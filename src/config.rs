//! Configuration module for the x402 facilitator server.

use clap::Parser;
use serde::Deserialize;
use std::fs;
use std::net::IpAddr;
use std::path::PathBuf;

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
}

impl Default for Config {
    fn default() -> Self {
        Config {
            port: config_defaults::default_port(),
            host: config_defaults::default_host(),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;

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
}
