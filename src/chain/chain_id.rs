use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;

use crate::networks;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ChainId {
    pub namespace: String,
    pub reference: String,
}

impl ChainId {
    pub fn new<N: Into<String>, R: Into<String>>(namespace: N, reference: R) -> Self {
        Self {
            namespace: namespace.into(),
            reference: reference.into(),
        }
    }

    pub fn namespace(&self) -> &str {
        &self.namespace
    }

    pub fn reference(&self) -> &str {
        &self.reference
    }

    /// Create a ChainId from a network name using the known v1 networks list
    pub fn from_network_name(network_name: &str) -> Option<Self> {
        networks::chain_id_by_network_name(network_name).cloned()
    }

    /// Get the network name for this chain ID using the known v1 networks list
    pub fn as_network_name(&self) -> Option<&'static str> {
        networks::network_name_by_chain_id(self)
    }
}

impl fmt::Display for ChainId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.reference)
    }
}

impl From<ChainId> for String {
    fn from(value: ChainId) -> Self {
        value.to_string()
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid chain id format {0}")]
pub struct ChainIdFormatError(String);

impl FromStr for ChainId {
    type Err = ChainIdFormatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.splitn(2, ':').collect();
        if parts.len() != 2 {
            return Err(ChainIdFormatError(s.into()));
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

#[derive(Debug, Clone)]
pub enum ChainIdPattern {
    Wildcard {
        namespace: String,
    },
    Exact {
        namespace: String,
        reference: String,
    },
    Set {
        namespace: String,
        references: HashSet<String>,
    },
}

impl ChainIdPattern {
    pub fn wildcard<S: Into<String>>(namespace: S) -> Self {
        Self::Wildcard {
            namespace: namespace.into(),
        }
    }

    pub fn exact<N: Into<String>, R: Into<String>>(namespace: N, reference: R) -> Self {
        Self::Exact {
            namespace: namespace.into(),
            reference: reference.into(),
        }
    }

    pub fn set(namespace: String, references: HashSet<String>) -> Self {
        Self::Set {
            namespace,
            references,
        }
    }

    /// Check if a `ChainId` matches this pattern.
    ///
    /// - `Wildcard` matches any chain with the same namespace
    /// - `Exact` matches only if both namespace and reference are equal
    /// - `Set` matches if the namespace is equal and the reference is in the set
    pub fn matches(&self, chain_id: &ChainId) -> bool {
        match self {
            ChainIdPattern::Wildcard { namespace } => chain_id.namespace == *namespace,
            ChainIdPattern::Exact {
                namespace,
                reference,
            } => chain_id.namespace == *namespace && chain_id.reference == *reference,
            ChainIdPattern::Set {
                namespace,
                references,
            } => chain_id.namespace == *namespace && references.contains(&chain_id.reference),
        }
    }

    /// Returns the namespace of this pattern.
    #[allow(dead_code)]
    pub fn namespace(&self) -> &str {
        match self {
            ChainIdPattern::Wildcard { namespace } => namespace,
            ChainIdPattern::Exact { namespace, .. } => namespace,
            ChainIdPattern::Set { namespace, .. } => namespace,
        }
    }
}

impl fmt::Display for ChainIdPattern {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ChainIdPattern::Wildcard { namespace } => write!(f, "{}:*", namespace),
            ChainIdPattern::Exact {
                namespace,
                reference,
            } => write!(f, "{}:{}", namespace, reference),
            ChainIdPattern::Set {
                namespace,
                references,
            } => {
                let refs: Vec<&str> = references.iter().map(|s| s.as_ref()).collect();
                write!(f, "{}:{{{}}}", namespace, refs.join(","))
            }
        }
    }
}

impl FromStr for ChainIdPattern {
    type Err = ChainIdFormatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (namespace, rest) = s.split_once(':').ok_or(ChainIdFormatError(s.into()))?;

        if namespace.is_empty() {
            return Err(ChainIdFormatError(s.into()));
        }

        // Wildcard: eip155:*
        if rest == "*" {
            return Ok(ChainIdPattern::wildcard(namespace));
        }

        // Set: eip155:{1,2,3}
        if let Some(inner) = rest.strip_prefix('{').and_then(|r| r.strip_suffix('}')) {
            let mut references = HashSet::new();

            for item in inner.split(',') {
                let item = item.trim();
                if item.is_empty() {
                    return Err(ChainIdFormatError(s.into()));
                }
                references.insert(item.into());
            }

            if references.is_empty() {
                return Err(ChainIdFormatError(s.into()));
            }

            return Ok(ChainIdPattern::set(namespace.into(), references));
        }

        // Exact: eip155:1
        if rest.is_empty() {
            return Err(ChainIdFormatError(s.into()));
        }

        Ok(ChainIdPattern::exact(namespace, rest))
    }
}

impl Serialize for ChainIdPattern {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for ChainIdPattern {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        ChainIdPattern::from_str(&s).map_err(de::Error::custom)
    }
}

impl From<ChainId> for ChainIdPattern {
    fn from(chain_id: ChainId) -> Self {
        ChainIdPattern::exact(chain_id.namespace, chain_id.reference)
    }
}

impl From<ChainIdPattern> for Vec<ChainIdPattern> {
    fn from(value: ChainIdPattern) -> Self {
        vec![value]
    }
}

impl From<ChainId> for Vec<ChainId> {
    fn from(value: ChainId) -> Self {
        vec![value]
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
        assert_eq!(chain_id.namespace, "eip155");
        assert_eq!(chain_id.reference, "1");
    }

    #[test]
    fn test_chain_id_deserialize_solana() {
        let chain_id: ChainId =
            serde_json::from_str("\"solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp\"").unwrap();
        assert_eq!(chain_id.namespace, "solana");
        assert_eq!(chain_id.reference, "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
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

    #[test]
    fn test_pattern_wildcard_matches() {
        let pattern = ChainIdPattern::wildcard("eip155".into());
        assert!(pattern.matches(&ChainId::new("eip155", "1")));
        assert!(pattern.matches(&ChainId::new("eip155", "8453")));
        assert!(pattern.matches(&ChainId::new("eip155", "137")));
        assert!(!pattern.matches(&ChainId::new("solana", "mainnet")));
    }

    #[test]
    fn test_pattern_exact_matches() {
        let pattern = ChainIdPattern::exact("eip155".into(), "1".into());
        assert!(pattern.matches(&ChainId::new("eip155", "1")));
        assert!(!pattern.matches(&ChainId::new("eip155", "8453")));
        assert!(!pattern.matches(&ChainId::new("solana", "1")));
    }

    #[test]
    fn test_pattern_set_matches() {
        let references: HashSet<String> = vec!["1", "8453", "137"]
            .into_iter()
            .map(String::from)
            .collect();
        let pattern = ChainIdPattern::set("eip155".into(), references);
        assert!(pattern.matches(&ChainId::new("eip155", "1")));
        assert!(pattern.matches(&ChainId::new("eip155", "8453")));
        assert!(pattern.matches(&ChainId::new("eip155", "137")));
        assert!(!pattern.matches(&ChainId::new("eip155", "42")));
        assert!(!pattern.matches(&ChainId::new("solana", "1")));
    }

    #[test]
    fn test_pattern_namespace() {
        let wildcard = ChainIdPattern::wildcard("eip155".into());
        assert_eq!(wildcard.namespace(), "eip155");

        let exact = ChainIdPattern::exact("solana".into(), "mainnet".into());
        assert_eq!(exact.namespace(), "solana");

        let references: HashSet<String> = vec!["1"].into_iter().map(String::from).collect();
        let set = ChainIdPattern::set("eip155".into(), references);
        assert_eq!(set.namespace(), "eip155");
    }
}
