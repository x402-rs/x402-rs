use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChainId {
    pub namespace: Box<str>,
    pub reference: Box<str>,
}

impl ChainId {
    pub fn new<N: Into<Box<str>>, R: Into<Box<str>>>(namespace: N, reference: R) -> Self {
        Self {
            namespace: namespace.into(),
            reference: reference.into(),
        }
    }

    pub fn namespace(&self) -> &str {
        self.namespace.as_ref()
    }

    pub fn reference(&self) -> &str {
        self.reference.as_ref()
    }
}

impl fmt::Display for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.reference)
    }
}

impl Into<String> for ChainId {
    fn into(self) -> String {
        self.to_string()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("invalid chain id format {0}")]
pub enum ChainIdError {
    #[error("invalid chain id format {0}")]
    InvalidFormat(Box<str>),
    #[error("unexpected namespace {0}, expected {1}")]
    UnexpectedNamespace(Box<str>, Box<str>),
    #[error("invalid chain id reference {0} for namespace {1}: {2}")]
    InvalidReference(Box<str>, Box<str>, Box<str>),
}

impl FromStr for ChainId {
    type Err = ChainIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(ChainIdError::InvalidFormat(s.into()));
        }
        Ok(ChainId {
            namespace: parts[0].into(),
            reference: parts[1].into(),
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
        let chain_id = ChainId::new("eip155", "1");
        let serialized = serde_json::to_string(&chain_id).unwrap();
        assert_eq!(serialized, "\"eip155:1\"");
    }

    #[test]
    fn test_chain_id_serialize_solana() {
        let chain_id = ChainId::new("solana", "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
        let serialized = serde_json::to_string(&chain_id).unwrap();
        assert_eq!(serialized, "\"solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp\"");
    }

    #[test]
    fn test_chain_id_deserialize_eip155() {
        let chain_id: ChainId = serde_json::from_str("\"eip155:1\"").unwrap();
        assert_eq!(chain_id.namespace.as_ref(), "eip155");
        assert_eq!(chain_id.reference.as_ref(), "1");
    }

    #[test]
    fn test_chain_id_deserialize_solana() {
        let chain_id: ChainId =
            serde_json::from_str("\"solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp\"").unwrap();
        assert_eq!(chain_id.namespace.as_ref(), "solana");
        assert_eq!(
            chain_id.reference.as_ref(),
            "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"
        );
    }

    #[test]
    fn test_chain_id_roundtrip_eip155() {
        let original = ChainId::new("eip155", "8453");
        // let original = ChainId::eip155(8453);
        let serialized = serde_json::to_string(&original).unwrap();
        let deserialized: ChainId = serde_json::from_str(&serialized).unwrap();
        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_chain_id_roundtrip_solana() {
        let original = ChainId::new("solana", "devnet");
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
        assert!(result.is_ok());
    }
}
