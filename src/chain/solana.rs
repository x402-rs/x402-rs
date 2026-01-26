//! Solana chain support for x402 payments.
//!
//! This module provides types and providers for interacting with the Solana blockchain
//! in the x402 protocol. It supports SPL token transfers for payment settlement.
//!
//! # Key Types
//!
//! - [`SolanaChainReference`] - A 32-character genesis hash identifying a Solana network
//! - [`SolanaChainProvider`] - Provider for interacting with Solana chains
//! - [`SolanaTokenDeployment`] - Token deployment information including mint address and decimals
//! - [`Address`] - A Solana public key (base58-encoded)
//!
//! # Solana Networks
//!
//! Solana networks are identified by the first 32 characters of their genesis block hash:
//! - Mainnet: `5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp`
//! - Devnet: `EtWTRABZaYq6iMfeYKouRu166VU2xqa1`
//!
//! # Example
//!
//! ```ignore
//! use x402_rs::chain::solana::{SolanaChainReference, SolanaTokenDeployment};
//! use x402_rs::networks::{KnownNetworkSolana, USDC};
//!
//! // Get USDC deployment on Solana mainnet
//! let usdc = USDC::solana();
//! assert_eq!(usdc.decimals, 6);
//!
//! // Parse a human-readable amount
//! let amount = usdc.parse("10.50").unwrap();
//! // amount.amount is now 10_500_000 (10.50 * 10^6)
//! ```

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
use x402_types::chain::ChainId;
use x402_types::util::money_amount::{MoneyAmount, MoneyAmountParseError};

use crate::chain::{ChainProviderOps, DeployedTokenAmount, FromConfig};
use crate::config::SolanaChainConfig;
use crate::networks::KnownNetworkSolana;
use crate::scheme::X402SchemeFacilitatorError;

/// The CAIP-2 namespace for Solana chains.
pub const SOLANA_NAMESPACE: &str = "solana";

/// A Solana chain reference consisting of 32 ASCII characters.
///
/// The reference is the first 32 characters of the base58-encoded genesis block hash,
/// which uniquely identifies a Solana network. This follows the CAIP-2 standard for
/// Solana chain identification.
///
/// # Well-Known References
///
/// - Mainnet: `5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp`
/// - Devnet: `EtWTRABZaYq6iMfeYKouRu166VU2xqa1`
///
/// # Example
///
/// ```
/// use x402_rs::chain::solana::SolanaChainReference;
/// use x402_rs::networks::KnownNetworkSolana;
///
/// let mainnet = SolanaChainReference::solana();
/// assert_eq!(mainnet.as_str(), "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SolanaChainReference([u8; 32]);

impl SolanaChainReference {
    /// Creates a new [`SolanaChainReference`] from a 32-byte ASCII array.
    ///
    /// # Panics
    ///
    /// This function does not validate that the bytes are valid ASCII.
    /// Use [`FromStr`] for validated parsing.
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

/// Error type for parsing Solana chain references.
#[derive(Debug, thiserror::Error)]
pub enum SolanaChainReferenceFormatError {
    /// The namespace was not "solana".
    #[error("Invalid namespace {0}, expected solana")]
    InvalidNamespace(String),
    /// The reference was not a valid 32-character ASCII string.
    #[error("Invalid solana chain reference {0}")]
    InvalidReference(String),
}

/// Information about an SPL token deployment on a Solana network.
///
/// This type contains all the information needed to interact with a specific
/// token on a specific Solana network, including the mint address and decimal
/// precision.
///
/// # Example
///
/// ```rust
/// use x402_rs::chain::solana::{SolanaChainReference, SolanaTokenDeployment, Address};
/// use x402_rs::networks::KnownNetworkSolana;
/// use std::str::FromStr;
///
/// // USDC on Solana mainnet
/// let usdc = SolanaTokenDeployment::new(
///     SolanaChainReference::solana(),
///     Address::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(),
///     6,
/// );
///
/// // Parse a human-readable amount
/// let amount = usdc.parse("10.50").unwrap();
/// assert_eq!(amount.amount, 10_500_000); // 10.50 * 10^6
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct SolanaTokenDeployment {
    /// The Solana network where this token is deployed.
    pub chain_reference: SolanaChainReference,
    /// The SPL token mint address.
    pub address: Address,
    /// The number of decimal places for this token.
    pub decimals: u8,
}

impl SolanaTokenDeployment {
    /// Creates a new token deployment.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn new(chain_reference: SolanaChainReference, address: Address, decimals: u8) -> Self {
        Self {
            chain_reference,
            address,
            decimals,
        }
    }

    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn amount(&self, v: u64) -> DeployedTokenAmount<u64, SolanaTokenDeployment> {
        DeployedTokenAmount {
            amount: v,
            token: self.clone(),
        }
    }

    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn parse<V>(
        &self,
        v: V,
    ) -> Result<DeployedTokenAmount<u64, SolanaTokenDeployment>, MoneyAmountParseError>
    where
        V: TryInto<MoneyAmount>,
        MoneyAmountParseError: From<<V as TryInto<MoneyAmount>>::Error>,
    {
        let money_amount = v.try_into()?;
        let scale = money_amount.scale();
        let token_scale = self.decimals as u32;
        if scale > token_scale {
            return Err(MoneyAmountParseError::WrongPrecision {
                money: scale,
                token: token_scale,
            });
        }
        let scale_diff = token_scale - scale;
        let multiplier = 10u64
            .checked_pow(scale_diff)
            .ok_or(MoneyAmountParseError::OutOfRange)?;
        let digits = u64::try_from(money_amount.mantissa()).expect("mantissa fits in u64");
        let value = digits
            .checked_mul(multiplier)
            .ok_or(MoneyAmountParseError::OutOfRange)?;
        Ok(DeployedTokenAmount {
            amount: value,
            token: self.clone(),
        })
    }
}

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
        Address(self.keypair.pubkey())
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

/// A Solana public key address.
///
/// This is a wrapper around [`Pubkey`] that provides serialization as a
/// base58-encoded string, suitable for use in x402 protocol messages.
///
/// # Example
///
/// ```
/// use x402_rs::chain::solana::Address;
/// use std::str::FromStr;
///
/// let addr = Address::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap();
/// assert_eq!(addr.to_string(), "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
/// ```
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Address(Pubkey);

impl Address {
    /// Creates a new address from a [`Pubkey`].
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

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_deployment(decimals: u8) -> SolanaTokenDeployment {
        let chain_ref = SolanaChainReference::solana();
        // Use a well-known test address (USDC on Solana devnet)
        let address = Address::from_str("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZ5nc4pb").unwrap();
        SolanaTokenDeployment::new(chain_ref, address, decimals)
    }

    #[test]
    fn test_parse_whole_number() {
        let deployment = create_test_deployment(6); // 6 decimals like USDC
        let result = deployment.parse("100");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, 100_000_000); // 100 * 10^6
    }

    #[test]
    fn test_parse_with_decimals() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("1.50");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, 1_500_000); // 1.50 * 10^6
    }

    #[test]
    fn test_parse_zero_decimals() {
        let deployment = create_test_deployment(0);
        let result = deployment.parse("42");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, 42);
    }

    #[test]
    fn test_parse_precision_too_high() {
        let deployment = create_test_deployment(2); // Only 2 decimals
        let result = deployment.parse("1.234"); // 3 decimals - should fail
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, MoneyAmountParseError::WrongPrecision { .. }));
    }

    #[test]
    fn test_parse_exact_precision() {
        let deployment = create_test_deployment(9); // 9 decimals
        let result = deployment.parse("0.123456789");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, 123_456_789);
    }

    #[test]
    fn test_parse_smallest_amount() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("0.000001");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, 1);
    }

    #[test]
    fn test_parse_with_currency_symbol() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("$10.50");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, 10_500_000);
    }

    #[test]
    fn test_parse_with_commas() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("1,000");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, 1_000_000_000);
    }

    #[test]
    fn test_parse_large_amount() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("999999999");
        assert!(result.is_ok());
        // 999999999 * 10^6 = 999999999000000
        assert_eq!(result.unwrap().amount, 999_999_999_000_000);
    }

    #[test]
    fn test_parse_overflow_returns_error() {
        // Create a deployment with 19 decimals (beyond what u64 can handle)
        let deployment = create_test_deployment(19);
        // 999999999 with 19 decimals = 999999999 * 10^19, which overflows u64
        let result = deployment.parse("999999999");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MoneyAmountParseError::OutOfRange
        ));
    }

    #[test]
    fn test_parse_matches_eip155_behavior() {
        use crate::chain::eip155::{Eip155ChainReference, Eip155TokenDeployment};

        // Create equivalent deployments
        let eip155_chain = Eip155ChainReference::new(1);
        let eip155_deployment = Eip155TokenDeployment {
            chain_reference: eip155_chain,
            address: alloy_primitives::Address::ZERO,
            decimals: 6,
            eip712: None,
        };

        let solana_deployment = create_test_deployment(6);

        // Test various amounts
        let test_cases = ["1", "1.5", "0.01", "100", "999.999"];

        for amount in test_cases {
            let eip155_result = eip155_deployment.parse(amount);
            let solana_result = solana_deployment.parse(amount);

            assert_eq!(eip155_result.is_ok(), solana_result.is_ok());

            if let (Ok(eip155), Ok(solana)) = (eip155_result, solana_result) {
                // EIP155 uses U256, Solana uses u64
                let eip155_value: u64 = eip155.amount.try_into().unwrap();
                assert_eq!(eip155_value, solana.amount);
            }
        }
    }
}
