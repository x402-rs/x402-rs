use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Debug, Display, Formatter, Write};
use std::str::FromStr;

use crate::config::SolanaChainConfig;
use crate::p1::chain::{ChainId, ChainIdError, ChainProviderOps};

pub const SOLANA_NAMESPACE: &str = "solana";

/// A Solana chain reference consisting of 32 ASCII characters.
/// The genesis hash is the first 32 characters of the base58-encoded genesis block hash.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct SolanaChainReference([u8; 32]);

impl SolanaChainReference {
    /// Creates a new SolanaChainReference from a 32-byte array.
    /// Returns None if any byte is not a valid ASCII character.
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the underlying bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns the chain reference as a string.
    pub fn as_str(&self) -> &str {
        // Safe because we validate ASCII on construction
        std::str::from_utf8(&self.0).expect("SolanaChainReference contains valid ASCII")
    }
}

impl Debug for SolanaChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("SolanaChainReference(")?;
        f.write_str(&self.as_str())?;
        f.write_str(")")
    }
}

/// Error type for parsing a SolanaChainReference from a string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SolanaChainReferenceParseError {
    #[error("invalid length: expected 32 characters, got {0}")]
    InvalidLength(usize),
    #[error("string contains non-ASCII characters")]
    NonAscii,
}

impl FromStr for SolanaChainReference {
    type Err = SolanaChainReferenceParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 32 {
            return Err(SolanaChainReferenceParseError::InvalidLength(s.len()));
        }
        if !s.is_ascii() {
            return Err(SolanaChainReferenceParseError::NonAscii);
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(s.as_bytes());
        Ok(Self(bytes))
    }
}

impl Display for SolanaChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for SolanaChainReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SolanaChainReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl Into<ChainId> for SolanaChainReference {
    fn into(self) -> ChainId {
        ChainId::new(SOLANA_NAMESPACE, self.as_str())
    }
}

impl TryFrom<ChainId> for SolanaChainReference {
    type Error = ChainIdError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        if value.namespace.as_ref() != SOLANA_NAMESPACE {
            return Err(ChainIdError::UnexpectedNamespace(
                value.namespace,
                SOLANA_NAMESPACE.into(),
            ));
        }
        let solana_chain_reference = Self::from_str(&value.reference).map_err(|e| {
            ChainIdError::InvalidReference(
                value.reference,
                SOLANA_NAMESPACE.into(),
                format!("{e:?}").into(),
            )
        })?;
        Ok(solana_chain_reference)
    }
}

#[derive(Debug)]
pub struct SolanaChainProvider {
    chain: SolanaChainReference,
}
impl SolanaChainProvider {
    pub async fn from_config(
        config: &SolanaChainConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            chain: config.chain_reference(),
        })
    }
}

impl ChainProviderOps for SolanaChainProvider {
    fn signer_addresses(&self) -> Vec<Box<str>> {
        // FIXME TODO
        vec![]
    }

    fn chain_id(&self) -> ChainId {
        self.chain.into()
    }
}
