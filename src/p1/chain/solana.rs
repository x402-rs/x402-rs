use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Debug, Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;
use solana_client::nonblocking::pubsub_client::PubsubClient;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::pubsub_client::PubsubClientError;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
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
        if value.namespace != SOLANA_NAMESPACE {
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

pub struct SolanaChainProvider {
    chain: SolanaChainReference,
    keypair: Arc<Keypair>,
    rpc_client: Arc<RpcClient>,
    pubsub_client: Arc<Option<PubsubClient>>,
    max_compute_unit_limit: u32,
    max_compute_unit_price: u64,
}

impl Debug for SolanaChainProvider {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolanaChainProvider")
            .field("pubkey", &self.keypair.pubkey())
            .field("chain", &self.chain)
            .field("rpc_url", &self.rpc_client.url())
            .finish()
    }
}

impl SolanaChainProvider {
    pub async fn from_config(
        config: &SolanaChainConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let rpc_url = config.rpc();
        let pubsub_url = config.pubsub().clone().map(|url| url.to_string());
        let keypair = Keypair::from_base58_string(&config.signer().to_string());
        let max_compute_unit_limit = config.max_compute_unit_limit();
        let max_compute_unit_price = config.max_compute_unit_price();
        let chain = config.chain_reference();
        let provider = Self::new(
            keypair,
            rpc_url.to_string(),
            pubsub_url,
            chain,
            max_compute_unit_limit,
            max_compute_unit_price,
        )
        .await?;
        Ok(provider)
    }

    pub async fn new(
        keypair: Keypair,
        rpc_url: String,
        pubsub_url: Option<String>,
        chain: SolanaChainReference,
        max_compute_unit_limit: u32,
        max_compute_unit_price: u64,
    ) -> Result<Self, PubsubClientError> {
        {
            let signer_addresses = vec![keypair.pubkey()];
            let chain_id: ChainId = chain.into();
            tracing::info!(
                chain = %chain_id,
                rpc = rpc_url,
                pubsub = ?pubsub_url,
                signers = ?signer_addresses,
                max_compute_unit_limit,
                max_compute_unit_price,
                "Initialized Solana provider"
            );
        }
        let rpc_client = RpcClient::new(rpc_url);
        let pubsub_client = if let Some(pubsub_url) = pubsub_url {
            let client = PubsubClient::new(pubsub_url).await?;
            Some(client)
        } else {
            None
        };
        Ok(Self {
            keypair: Arc::new(keypair),
            chain,
            rpc_client: Arc::new(rpc_client),
            pubsub_client: Arc::new(pubsub_client),
            max_compute_unit_limit,
            max_compute_unit_price,
        })
    }

    pub fn fee_payer(&self) -> Address {
        Address(self.keypair.pubkey())
    }
}

impl ChainProviderOps for SolanaChainProvider {
    fn signer_addresses(&self) -> Vec<String> {
        vec![self.fee_payer().to_string()]
    }

    fn chain_id(&self) -> ChainId {
        self.chain.into()
    }
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Address(Pubkey);

impl Address {
    pub const fn new(pubkey: Pubkey) -> Self {
        Self(pubkey)
    }
}

impl From<Pubkey> for Address {
    fn from(pubkey: Pubkey) -> Self {
        Self(pubkey)
    }
}

impl From<Address> for Pubkey {
    fn from(address: Address) -> Self {
        address.0
    }
}

impl Serialize for Address {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let base58_string = self.0.to_string();
        serializer.serialize_str(&base58_string)
    }
}

impl<'de> Deserialize<'de> for Address {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let pubkey = Pubkey::from_str(&s)
            .map_err(|_| serde::de::Error::custom("Failed to decode Solana address"))?;
        Ok(Self(pubkey))
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_string())
    }
}

impl FromStr for Address {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let pubkey =
            Pubkey::from_str(s).map_err(|_| format!("Failed to decode Solana address: {s}"))?;
        Ok(Self(pubkey))
    }
}
