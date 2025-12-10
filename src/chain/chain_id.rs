use serde::{de, Deserialize, Deserializer, Serialize, Serializer};
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

impl FromStr for Namespace {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Use serde_json to deserialize from the string value
        let json = format!("\"{}\"", s);
        serde_json::from_str(&json).map_err(|e| format!("unknown namespace '{}': {}", s, e))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChainId {
    pub namespace: Namespace,
    pub chain_id: String,
}

impl fmt::Display for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.chain_id)
    }
}

impl FromStr for ChainId {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(format!("invalid chain id format: {}", s));
        }
        let namespace = Namespace::from_str(parts[0])?;
        let chain_id = parts[1].to_string();
        Ok(ChainId { namespace, chain_id })
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
        let chain_id = ChainId {
            namespace: Namespace::Eip155,
            chain_id: "1".to_string(),
        };
        let serialized = serde_json::to_string(&chain_id).unwrap();
        assert_eq!(serialized, "\"eip155:1\"");
    }

    #[test]
    fn test_chain_id_serialize_solana() {
        let chain_id = ChainId {
            namespace: Namespace::Solana,
            chain_id: "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp".to_string(),
        };
        let serialized = serde_json::to_string(&chain_id).unwrap();
        assert_eq!(serialized, "\"solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp\"");
    }

    #[test]
    fn test_chain_id_deserialize_eip155() {
        let chain_id: ChainId = serde_json::from_str("\"eip155:1\"").unwrap();
        assert_eq!(chain_id.namespace, Namespace::Eip155);
        assert_eq!(chain_id.chain_id, "1");
    }

    #[test]
    fn test_chain_id_deserialize_solana() {
        let chain_id: ChainId = serde_json::from_str("\"solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp\"").unwrap();
        assert_eq!(chain_id.namespace, Namespace::Solana);
        assert_eq!(chain_id.chain_id, "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
    }

    #[test]
    fn test_chain_id_roundtrip_eip155() {
        let original = ChainId {
            namespace: Namespace::Eip155,
            chain_id: "8453".to_string(),
        };
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: ChainId = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_chain_id_roundtrip_solana() {
        let original = ChainId {
            namespace: Namespace::Solana,
            chain_id: "devnet".to_string(),
        };
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