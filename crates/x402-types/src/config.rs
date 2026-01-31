//! Configuration types for x402 infrastructure.
//!
//! This module provides configuration types used throughout the x402 ecosystem,
//! including RPC provider configuration and environment variable resolution.
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

use serde::{Deserialize, Serialize};
use std::ops::{Deref, DerefMut};
use std::str::FromStr;
use url::Url;

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
