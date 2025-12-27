use serde::{Deserialize, Deserializer, Serialize, Serializer};
use solana_account::Account;
use solana_client::client_error::{ClientError, ClientErrorKind};
use solana_client::nonblocking::pubsub_client::PubsubClient;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::pubsub_client::PubsubClientError;
use solana_client::rpc_client::SerializableTransaction;
use solana_client::rpc_config::{
    RpcSendTransactionConfig, RpcSignatureSubscribeConfig, RpcSimulateTransactionConfig,
};
use solana_client::rpc_response::{RpcSignatureResult, UiTransactionError};
use solana_commitment_config::CommitmentConfig;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_signer::{Signer, SignerError};
use solana_transaction::TransactionError;
use solana_transaction::versioned::VersionedTransaction;
use std::fmt::{Debug, Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use crate::chain::{ChainId, ChainProviderOps};
use crate::config::SolanaChainConfig;
use crate::networks::KnownNetworkSolana;
use crate::scheme::X402SchemeFacilitatorError;

pub const SOLANA_NAMESPACE: &str = "solana";

/// A Solana chain reference consisting of 32 ASCII characters.
/// The genesis hash is the first 32 characters of the base58-encoded genesis block hash.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SolanaChainReference([u8; 32]);

impl SolanaChainReference {
    /// Creates a new SolanaChainReference from a 32-byte array.
    /// Returns None if any byte is not a valid ASCII character.
    #[allow(dead_code)]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the underlying bytes.
    #[allow(dead_code)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns the chain reference as a string.
    pub fn as_str(&self) -> &str {
        // Safe because we validate ASCII on construction
        std::str::from_utf8(&self.0).expect("SolanaChainReference contains valid ASCII")
    }
}

impl KnownNetworkSolana<SolanaChainReference> for SolanaChainReference {
    fn solana() -> Self {
        Self::new(*b"5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp")
    }

    fn solana_devnet() -> Self {
        Self::new(*b"EtWTRABZaYq6iMfeYKouRu166VU2xqa1")
    }
}

impl Debug for SolanaChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("SolanaChainReference(")?;
        f.write_str(self.as_str())?;
        f.write_str(")")
    }
}

impl FromStr for SolanaChainReference {
    type Err = SolanaChainReferenceFormatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !(s.is_ascii() && s.len() == 32) {
            return Err(SolanaChainReferenceFormatError::InvalidReference(
                s.to_string(),
            ));
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

impl From<SolanaChainReference> for ChainId {
    fn from(value: SolanaChainReference) -> Self {
        ChainId::new(SOLANA_NAMESPACE, value.as_str())
    }
}

impl TryFrom<ChainId> for SolanaChainReference {
    type Error = SolanaChainReferenceFormatError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        if value.namespace != SOLANA_NAMESPACE {
            return Err(SolanaChainReferenceFormatError::InvalidNamespace(
                value.namespace,
            ));
        }
        let solana_chain_reference = Self::from_str(&value.reference)
            .map_err(|_| SolanaChainReferenceFormatError::InvalidReference(value.reference))?;
        Ok(solana_chain_reference)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SolanaChainReferenceFormatError {
    #[error("Invalid namespace {0}, expected solana")]
    InvalidNamespace(String),
    #[error("Invalid solana chain reference {0}")]
    InvalidReference(String),
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct SolanaTokenDeployment {
    pub chain_reference: SolanaChainReference,
    pub address: Address,
    pub decimals: u8,
}

impl SolanaTokenDeployment {
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn new(chain_reference: SolanaChainReference, address: Address, decimals: u8) -> Self {
        Self {
            chain_reference,
            address,
            decimals,
        }
    }
}

#[derive(thiserror::Error, Debug)]
pub enum SolanaChainProviderError {
    #[error(transparent)]
    Signer(#[from] SignerError),
    #[error("Invalid transaction: {0}")]
    InvalidTransaction(#[from] UiTransactionError),
    #[error(transparent)]
    Transport(Box<ClientErrorKind>),
    #[error(transparent)]
    PubsubTransport(#[from] PubsubClientError),
}

impl From<ClientError> for SolanaChainProviderError {
    fn from(value: ClientError) -> Self {
        SolanaChainProviderError::Transport(value.kind)
    }
}

impl From<SolanaChainProviderError> for X402SchemeFacilitatorError {
    fn from(value: SolanaChainProviderError) -> Self {
        Self::OnchainFailure(value.to_string())
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

    pub fn max_compute_unit_limit(&self) -> u32 {
        self.max_compute_unit_limit
    }

    pub fn max_compute_unit_price(&self) -> u64 {
        self.max_compute_unit_price
    }

    pub fn pubkey(&self) -> Pubkey {
        self.keypair.pubkey()
    }

    pub fn sign(
        &self,
        tx: VersionedTransaction,
    ) -> Result<VersionedTransaction, SolanaChainProviderError> {
        let mut tx = tx.clone();
        let msg_bytes = tx.message.serialize();
        let signature = self.keypair.try_sign_message(msg_bytes.as_slice())?;
        // Required signatures are the first N account keys
        let num_required = tx.message.header().num_required_signatures as usize;
        let static_keys = tx.message.static_account_keys();
        // Find signer’s position
        let pos = static_keys[..num_required]
            .iter()
            .position(|k| *k == self.pubkey())
            .ok_or(SolanaChainProviderError::InvalidTransaction(
                UiTransactionError::from(TransactionError::InvalidAccountIndex),
            ))?;
        // Ensure signature vector is large enough, then place the signature
        if tx.signatures.len() < num_required {
            tx.signatures.resize(num_required, Signature::default());
        }
        // tx.signatures.push(signature);
        tx.signatures[pos] = signature;
        Ok(tx)
    }

    pub async fn simulate_transaction_with_config(
        &self,
        tx: &VersionedTransaction,
        cfg: RpcSimulateTransactionConfig,
    ) -> Result<(), SolanaChainProviderError> {
        let sim = self
            .rpc_client
            .simulate_transaction_with_config(tx, cfg)
            .await?;
        match sim.value.err {
            None => Ok(()),
            Some(e) => Err(SolanaChainProviderError::InvalidTransaction(e)),
        }
    }

    pub async fn get_multiple_accounts(
        &self,
        pubkeys: &[Pubkey],
    ) -> Result<Vec<Option<Account>>, SolanaChainProviderError> {
        let accounts = self.rpc_client.get_multiple_accounts(pubkeys).await?;
        Ok(accounts)
    }

    pub async fn send(
        &self,
        tx: &VersionedTransaction,
    ) -> Result<Signature, SolanaChainProviderError> {
        let signature = self
            .rpc_client
            .send_transaction_with_config(
                tx,
                RpcSendTransactionConfig {
                    skip_preflight: true,
                    ..RpcSendTransactionConfig::default()
                },
            )
            .await?;
        Ok(signature)
    }

    pub async fn send_and_confirm(
        &self,
        tx: &VersionedTransaction,
        commitment_config: CommitmentConfig,
    ) -> Result<Signature, SolanaChainProviderError> {
        let tx_sig = tx.get_signature();

        use futures_util::stream::StreamExt;

        if let Some(pubsub_client) = self.pubsub_client.as_ref() {
            let config = RpcSignatureSubscribeConfig {
                commitment: Some(commitment_config),
                enable_received_notification: None,
            };
            let (mut stream, unsubscribe) = pubsub_client
                .signature_subscribe(tx_sig, Some(config))
                .await?;
            if let Err(e) = self.send(tx).await {
                tracing::error!(error = %e, "Failed to send transaction");
                unsubscribe().await;
                return Err(e);
            }
            if let Some(response) = stream.next().await {
                let error = if let RpcSignatureResult::ProcessedSignature(r) = response.value {
                    r.err
                } else {
                    None
                };
                match error {
                    None => Ok(*tx_sig),
                    Some(error) => Err(SolanaChainProviderError::InvalidTransaction(error)),
                }
            } else {
                Err(SolanaChainProviderError::Transport(Box::new(
                    ClientErrorKind::Custom(
                        "Can not get response from signatureSubscribe".to_string(),
                    ),
                )))
            }
        } else {
            self.send(tx).await?;
            loop {
                let confirmed = self
                    .rpc_client
                    .confirm_transaction_with_commitment(tx_sig, commitment_config)
                    .await?;
                if confirmed.value {
                    return Ok(*tx_sig);
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
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

    pub fn pubkey(&self) -> &Pubkey {
        &self.0
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

impl AsRef<[u8]> for Address {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
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
        write!(f, "{}", self.0)
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
