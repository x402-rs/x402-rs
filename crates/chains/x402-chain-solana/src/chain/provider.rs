#![cfg(feature = "facilitator")]

use solana_account::Account;
use solana_client::client_error::{ClientError, ClientErrorKind};
use solana_client::nonblocking::pubsub_client::PubsubClient;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::pubsub_client::PubsubClientError;
use solana_client::rpc_client::SerializableTransaction;
use solana_client::rpc_config::{
    RpcSendTransactionConfig, RpcSignatureSubscribeConfig, RpcSimulateTransactionConfig,
};
use solana_client::rpc_response::{RpcSignatureResult, TransactionError, UiTransactionError};
use solana_commitment_config::CommitmentConfig;
use solana_keypair::Keypair;
use solana_keypair::Signer;
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_signer::SignerError;
use solana_transaction::versioned::VersionedTransaction;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use std::time::Duration;
use x402_types::chain::{ChainId, ChainProviderOps, FromConfig};
use x402_types::proto::PaymentVerificationError;
use x402_types::scheme::X402SchemeFacilitatorError;

use crate::chain::config::SolanaChainConfig;
use crate::chain::types::{Address, SolanaChainReference};

/// Errors that can occur when interacting with a Solana chain provider.
#[derive(thiserror::Error, Debug)]
pub enum SolanaChainProviderError {
    /// Failed to sign a transaction.
    #[error(transparent)]
    Signer(#[from] SignerError),
    /// The transaction was invalid or failed simulation.
    #[error("Invalid transaction: {0}")]
    InvalidTransaction(#[from] UiTransactionError),
    /// RPC transport error.
    #[error(transparent)]
    Transport(Box<ClientErrorKind>),
    /// WebSocket pubsub transport error.
    #[error(transparent)]
    PubsubTransport(#[from] PubsubClientError),
    #[error("{0}")]
    #[allow(dead_code)] // Public for consumption by downstream crates.
    Custom(String),
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

impl From<SolanaChainProviderError> for PaymentVerificationError {
    fn from(value: SolanaChainProviderError) -> Self {
        Self::TransactionSimulation(value.to_string())
    }
}

/// Provider for interacting with a Solana blockchain.
///
/// This provider handles transaction signing, simulation, and submission for
/// Solana-based x402 payments. It supports both RPC polling and WebSocket
/// subscriptions for transaction confirmation.
///
/// # Configuration
///
/// The provider requires:
/// - A keypair for signing transactions (the fee payer)
/// - An RPC endpoint URL
/// - Optionally, a WebSocket pubsub URL for faster confirmations
/// - Compute unit limits and prices for transaction prioritization
///
/// # Example
///
/// ```ignore
/// use x402::chain::solana::SolanaChainProvider;
/// use x402::config::SolanaChainConfig;
///
/// let provider = SolanaChainProvider::from_config(&config).await?;
/// println!("Fee payer: {}", provider.fee_payer());
/// ```
pub struct SolanaChainProvider {
    /// The Solana network this provider connects to.
    chain: SolanaChainReference,
    /// The keypair used for signing transactions.
    keypair: Arc<Keypair>,
    /// The RPC client for sending requests.
    rpc_client: Arc<RpcClient>,
    /// Optional WebSocket client for subscriptions.
    pubsub_client: Option<Arc<PubsubClient>>,
    /// Maximum compute units allowed per transaction.
    max_compute_unit_limit: u32,
    /// Maximum price per compute unit (in micro-lamports).
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
    pub async fn new(
        keypair: Keypair,
        rpc_url: String,
        pubsub_url: Option<String>,
        chain: SolanaChainReference,
        max_compute_unit_limit: u32,
        max_compute_unit_price: u64,
    ) -> Result<Self, PubsubClientError> {
        #[cfg(feature = "telemetry")]
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
                "Using Solana provider"
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
            pubsub_client: pubsub_client.map(Arc::new),
            max_compute_unit_limit,
            max_compute_unit_price,
        })
    }

    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn rpc_client(&self) -> Arc<RpcClient> {
        Arc::clone(&self.rpc_client)
    }

    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn pubsub_client(&self) -> Option<Arc<PubsubClient>> {
        self.pubsub_client.clone()
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
}

#[async_trait::async_trait]
impl FromConfig<SolanaChainConfig> for SolanaChainProvider {
    async fn from_config(config: &SolanaChainConfig) -> Result<Self, Box<dyn std::error::Error>> {
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
}

impl ChainProviderOps for SolanaChainProvider {
    fn signer_addresses(&self) -> Vec<String> {
        vec![self.fee_payer().to_string()]
    }

    fn chain_id(&self) -> ChainId {
        self.chain.into()
    }
}

pub trait SolanaChainProviderLike {
    fn simulate_transaction_with_config(
        &self,
        tx: &VersionedTransaction,
        cfg: RpcSimulateTransactionConfig,
    ) -> impl Future<Output = Result<(), SolanaChainProviderError>> + Send;
    fn get_multiple_accounts(
        &self,
        pubkeys: &[Pubkey],
    ) -> impl Future<Output = Result<Vec<Option<Account>>, SolanaChainProviderError>> + Send;
    fn max_compute_unit_limit(&self) -> u32;
    fn max_compute_unit_price(&self) -> u64;
    fn pubkey(&self) -> Pubkey;
    fn fee_payer(&self) -> Address;
    fn sign(
        &self,
        tx: VersionedTransaction,
    ) -> Result<VersionedTransaction, SolanaChainProviderError>;
    fn send_and_confirm(
        &self,
        tx: &VersionedTransaction,
        commitment_config: CommitmentConfig,
    ) -> impl Future<Output = Result<Signature, SolanaChainProviderError>> + Send;
}

impl SolanaChainProviderLike for SolanaChainProvider {
    async fn simulate_transaction_with_config(
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

    async fn get_multiple_accounts(
        &self,
        pubkeys: &[Pubkey],
    ) -> Result<Vec<Option<Account>>, SolanaChainProviderError> {
        let accounts = self.rpc_client.get_multiple_accounts(pubkeys).await?;
        Ok(accounts)
    }

    fn max_compute_unit_limit(&self) -> u32 {
        self.max_compute_unit_limit
    }

    fn max_compute_unit_price(&self) -> u64 {
        self.max_compute_unit_price
    }

    fn pubkey(&self) -> Pubkey {
        self.keypair.pubkey()
    }

    fn fee_payer(&self) -> Address {
        Address::new(self.keypair.pubkey())
    }

    fn sign(
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

    async fn send_and_confirm(
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
                #[cfg(feature = "telemetry")]
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

impl<T: SolanaChainProviderLike> SolanaChainProviderLike for Arc<T> {
    fn simulate_transaction_with_config(
        &self,
        tx: &VersionedTransaction,
        cfg: RpcSimulateTransactionConfig,
    ) -> impl Future<Output = Result<(), SolanaChainProviderError>> + Send {
        (**self).simulate_transaction_with_config(tx, cfg)
    }

    fn get_multiple_accounts(
        &self,
        pubkeys: &[Pubkey],
    ) -> impl Future<Output = Result<Vec<Option<Account>>, SolanaChainProviderError>> + Send {
        (**self).get_multiple_accounts(pubkeys)
    }

    fn max_compute_unit_limit(&self) -> u32 {
        (**self).max_compute_unit_limit()
    }

    fn max_compute_unit_price(&self) -> u64 {
        (**self).max_compute_unit_price()
    }

    fn pubkey(&self) -> Pubkey {
        (**self).pubkey()
    }

    fn fee_payer(&self) -> Address {
        (**self).fee_payer()
    }

    fn sign(
        &self,
        tx: VersionedTransaction,
    ) -> Result<VersionedTransaction, SolanaChainProviderError> {
        (**self).sign(tx)
    }

    fn send_and_confirm(
        &self,
        tx: &VersionedTransaction,
        commitment_config: CommitmentConfig,
    ) -> impl Future<Output = Result<Signature, SolanaChainProviderError>> + Send {
        (**self).send_and_confirm(tx, commitment_config)
    }
}
