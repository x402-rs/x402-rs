use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Namespace {
    Solana,
    Eip155,
}

impl fmt::Display for Namespace {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Use serde_json to get the serialized string value
        let json = serde_json::to_string(self).map_err(|_| fmt::Error)?;
        // Remove the surrounding quotes from JSON string
        let s = json.trim_matches('"');
        write!(f, "{}", s)
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

#[derive(Clone, PartialEq, Eq, Hash)]
pub struct ChainId {
    pub namespace: String,
    pub reference: String,
}

impl ChainId {
    pub fn eip155(chain_id: u64) -> Self {
        Self {
            namespace: Namespace::Eip155.to_string(),
            reference: chain_id.to_string(),
        }
    }

    pub fn solana(chain_id: &str) -> Self {
        Self {
            namespace: Namespace::Solana.to_string(),
            reference: chain_id.to_string(),
        }
    }
}

impl TryInto<Namespace> for ChainId {
    type Error = UnsupportedNamespaceError;

    fn try_into(self) -> Result<Namespace, Self::Error> {
        Namespace::from_str(&self.namespace)
    }
}

impl fmt::Display for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.reference)
    }
}

impl fmt::Debug for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ChainIdError {
    #[error("invalid chain id format {0}")]
    InvalidFormat(String),
    #[error("unexpected namespace {0}, expected {1}")]
    UnexpectedNamespace(String, Namespace),
    #[error("invalid chain id reference {0} for namespace {1}: {2}")]
    InvalidReference(String, Namespace, String),
}

impl FromStr for ChainId {
    type Err = ChainIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(ChainIdError::InvalidFormat(s.to_string()));
        }
        Ok(ChainId {
            namespace: parts[0].to_string(),
            reference: parts[1].to_string(),
        })
    }
}

impl Serialize for ChainId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ChainId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        ChainId::from_str(&s).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_id_serialize_eip155() {
        let chain_id = ChainId::eip155(1);
        let serialized = serde_json::to_string(&chain_id).unwrap();
        assert_eq!(serialized, "\"eip155:1\"");
    }

    #[test]
    fn test_chain_id_serialize_solana() {
        let chain_id = ChainId::solana("5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
        let serialized = serde_json::to_string(&chain_id).unwrap();
        assert_eq!(serialized, "\"solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp\"");
    }

    #[test]
    fn test_chain_id_deserialize_eip155() {
        let chain_id: ChainId = serde_json::from_str("\"eip155:1\"").unwrap();
        assert_eq!(chain_id.namespace, Namespace::Eip155.to_string());
        assert_eq!(chain_id.reference, "1");
    }

    #[test]
    fn test_chain_id_deserialize_solana() {
        let chain_id: ChainId =
            serde_json::from_str("\"solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp\"").unwrap();
        assert_eq!(chain_id.namespace, Namespace::Solana.to_string());
        assert_eq!(chain_id.reference, "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
    }

    #[test]
    fn test_chain_id_roundtrip_eip155() {
        let original = ChainId::eip155(8453);
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: ChainId = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_chain_id_roundtrip_solana() {
        let original = ChainId::solana("devnet");
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: ChainId = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_chain_id_deserialize_invalid_format() {
        let result: Result<ChainId, _> = serde_json::from_str("\"invalid\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_chain_id_deserialize_unknown_namespace() {
        let result: Result<ChainId, _> = serde_json::from_str("\"unknown:1\"");
        assert!(result.is_err());
    }

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
