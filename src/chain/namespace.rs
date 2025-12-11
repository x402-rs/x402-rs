use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Namespace {
    Solana,
    Eip155,
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let json = serde_json::to_string(self).map_err(|_| fmt::Error)?;
        write!(f, "{}", json.trim_matches('"'))
    }
}

#[derive(Debug, thiserror::Error)]
#[error("unsupported namespace {0}")]
pub struct UnsupportedNamespaceError(String);

impl FromStr for Namespace {
    type Err = UnsupportedNamespaceError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Use serde_json to deserialize from the string value
        let json = format!("\"{}\"", s);
        serde_json::from_str(&json).map_err(|_| UnsupportedNamespaceError(s.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_namespace_display() {
        assert_eq!(Namespace::Eip155.to_string(), "eip155");
        assert_eq!(Namespace::Solana.to_string(), "solana");
    }

    #[test]
    fn test_namespace_from_str() {
        assert_eq!(Namespace::from_str("eip155").unwrap(), Namespace::Eip155);
        assert_eq!(Namespace::from_str("solana").unwrap(), Namespace::Solana);
        assert!(Namespace::from_str("unknown").is_err());
    }
}
