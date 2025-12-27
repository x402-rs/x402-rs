pub mod pending_nonce_manager;
pub mod types;

use alloy_network::{Ethereum as AlloyEthereum, EthereumWallet, NetworkWallet, TransactionBuilder};
use alloy_primitives::{Address, B256, Bytes};
use alloy_provider::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller,
};
use alloy_provider::{
    Identity, PendingTransactionError, Provider, ProviderBuilder, RootProvider, WalletProvider,
};
use alloy_rpc_client::RpcClient;
use alloy_rpc_types_eth::{BlockId, TransactionReceipt, TransactionRequest};
use alloy_signer::Signer;
use alloy_signer_local::PrivateKeySigner;
use alloy_transport::TransportError;
use alloy_transport::layers::{FallbackLayer, ThrottleLayer};
use alloy_transport_http::Http;
use std::fmt::{Display, Formatter};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceBuilder;
use tracing::Instrument;

use crate::chain::{ChainId, ChainProviderOps};
use crate::config::Eip155ChainConfig;
pub use pending_nonce_manager::*;
pub use types::*;

/// Combined filler type for gas, blob gas, nonce, and chain ID.
pub type InnerFiller = JoinFill<
    GasFiller,
    JoinFill<BlobGasFiller, JoinFill<NonceFiller<PendingNonceManager>, ChainIdFiller>>,
>;

/// The fully composed Ethereum provider type used in this project.
///
/// Combines multiple filler layers for gas, nonce, chain ID, blob gas, and wallet signing,
/// and wraps a [`RootProvider`] for actual JSON-RPC communication.
pub type InnerProvider = FillProvider<
    JoinFill<JoinFill<Identity, InnerFiller>, WalletFiller<EthereumWallet>>,
    RootProvider,
>;

pub const EIP155_NAMESPACE: &str = "eip155";

#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct Eip155ChainReference(u64);

impl Eip155ChainReference {
    pub fn as_chain_id(&self) -> ChainId {
        ChainId::new(EIP155_NAMESPACE, self.0.to_string())
    }
}

impl From<Eip155ChainReference> for ChainId {
    fn from(value: Eip155ChainReference) -> Self {
        ChainId::new(EIP155_NAMESPACE, value.0.to_string())
    }
}

impl From<&Eip155ChainReference> for ChainId {
    fn from(value: &Eip155ChainReference) -> Self {
        ChainId::new(EIP155_NAMESPACE, value.0.to_string())
    }
}

impl TryFrom<ChainId> for Eip155ChainReference {
    type Error = Eip155ChainReferenceFormatError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        if value.namespace != EIP155_NAMESPACE {
            return Err(Eip155ChainReferenceFormatError::InvalidNamespace(
                value.namespace,
            ));
        }
        let chain_id: u64 = value.reference.parse().map_err(|_| {
            Eip155ChainReferenceFormatError::InvalidReference(value.reference.clone())
        })?;
        Ok(Eip155ChainReference(chain_id))
    }
}

impl TryFrom<&ChainId> for Eip155ChainReference {
    type Error = Eip155ChainReferenceFormatError;

    fn try_from(value: &ChainId) -> Result<Self, Self::Error> {
        if value.namespace != EIP155_NAMESPACE {
            return Err(Eip155ChainReferenceFormatError::InvalidNamespace(
                value.namespace.clone(),
            ));
        }
        let chain_id: u64 = value.reference.parse().map_err(|_| {
            Eip155ChainReferenceFormatError::InvalidReference(value.reference.clone())
        })?;
        Ok(Eip155ChainReference(chain_id))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Eip155ChainReferenceFormatError {
    #[error("Invalid namespace {0}, expected eip155")]
    InvalidNamespace(String),
    #[error("Invalid eip155 chain reference {0}")]
    InvalidReference(String),
}

impl Eip155ChainReference {
    pub fn new(chain_id: u64) -> Self {
        Self(chain_id)
    }
    pub fn inner(&self) -> u64 {
        self.0
    }
}

impl Display for Eip155ChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct Eip155TokenDeployment {
    pub chain_reference: Eip155ChainReference,
    pub address: Address,
    pub decimals: u8,
    pub eip712: Option<TokenDeploymentEip712>,
}

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct TokenDeploymentEip712 {
    pub name: String,
    pub version: String,
}

#[derive(Debug)]
pub struct Eip155ChainProvider {
    chain: Eip155ChainReference,
    eip1559: bool,
    flashblocks: bool,
    receipt_timeout_secs: u64,
    inner: InnerProvider,
    /// Available signer addresses for round-robin selection.
    signer_addresses: Arc<Vec<Address>>,
    /// Current position in round-robin signer rotation.
    signer_cursor: Arc<AtomicUsize>,
    /// Nonce manager for resetting nonces on transaction failures.
    nonce_manager: PendingNonceManager,
}

impl Eip155ChainProvider {
    pub async fn from_config(
        config: &Eip155ChainConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // 1. Signers
        let signers = config
            .signers()
            .iter()
            .map(|s| B256::from_slice(s.inner().as_bytes()))
            .map(|b| {
                PrivateKeySigner::from_bytes(&b)
                    .map(|s| s.with_chain_id(Some(config.chain_reference().inner())))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if signers.is_empty() {
            return Err("at least one signer should be provided".into());
        }
        let wallet = {
            let mut iter = signers.into_iter();
            let first_signer = iter
                .next()
                .expect("iterator contains at least one element by construction");
            let mut wallet = EthereumWallet::from(first_signer);
            for signer in iter {
                wallet.register_signer(signer);
            }
            wallet
        };
        let signer_addresses =
            NetworkWallet::<AlloyEthereum>::signer_addresses(&wallet).collect::<Vec<_>>();
        let signer_addresses = Arc::new(signer_addresses);
        let signer_cursor = Arc::new(AtomicUsize::new(0));

        // 2. Transports
        let transports = config
            .rpc()
            .iter()
            .filter_map(|provider_config| {
                let scheme = provider_config.http.scheme();
                let is_http = scheme == "http" || scheme == "https";
                if !is_http {
                    return None;
                }
                let rpc_url = provider_config.http.clone();
                tracing::info!(chain=%config.chain_id(), rpc_url=%rpc_url, rate_limit=?provider_config.rate_limit, "Using HTTP transport");
                let rate_limit = provider_config.rate_limit.unwrap_or(u32::MAX);
                let service = ServiceBuilder::new()
                    .layer(ThrottleLayer::new(rate_limit))
                    .service(Http::new(rpc_url));
                Some(service)
            })
            .collect::<Vec<_>>();
        let fallback = ServiceBuilder::new()
            .layer(
                FallbackLayer::default().with_active_transport_count(
                    NonZeroUsize::new(transports.len())
                        .expect("Non-zero amount of stateless transports"),
                ),
            )
            .service(transports);
        let client = RpcClient::new(fallback, false);

        // 3. Provider
        // Create nonce manager explicitly so we can store a reference for error handling
        let nonce_manager = PendingNonceManager::default();
        // Build the filler stack: Gas -> BlobGas -> Nonce -> ChainId
        // This mirrors the InnerFiller type but with our custom nonce manager
        let filler = JoinFill::new(
            GasFiller,
            JoinFill::new(
                BlobGasFiller,
                JoinFill::new(
                    NonceFiller::new(nonce_manager.clone()),
                    ChainIdFiller::default(),
                ),
            ),
        );
        let inner: InnerProvider = ProviderBuilder::default()
            .filler(filler)
            .wallet(wallet)
            .connect_client(client);

        tracing::info!(chain=%config.chain_id(), signers=?signer_addresses, "Initialized EVM provider");

        Ok(Self {
            chain: config.chain_reference(),
            eip1559: config.eip1559(),
            flashblocks: config.flashblocks(),
            receipt_timeout_secs: config.receipt_timeout_secs(),
            inner,
            signer_addresses,
            signer_cursor,
            nonce_manager,
        })
    }

    /// Round-robin selection of next signer from wallet.
    fn next_signer_address(&self) -> Address {
        debug_assert!(!self.signer_addresses.is_empty());
        if self.signer_addresses.len() == 1 {
            self.signer_addresses[0]
        } else {
            let next =
                self.signer_cursor.fetch_add(1, Ordering::Relaxed) % self.signer_addresses.len();
            self.signer_addresses[next]
        }
    }
}

impl Eip155MetaTransactionProvider for &Eip155ChainProvider {
    type Error = MetaTransactionSendError;
    type Inner = InnerProvider;
    fn inner(&self) -> &Self::Inner {
        (*self).inner()
    }
    fn chain(&self) -> &Eip155ChainReference {
        (*self).chain()
    }
    fn send_transaction(
        &self,
        tx: MetaTransaction,
    ) -> impl Future<Output = Result<TransactionReceipt, Self::Error>> + Send {
        (*self).send_transaction(tx)
    }
}

impl Eip155MetaTransactionProvider for Eip155ChainProvider {
    type Error = MetaTransactionSendError;
    type Inner = InnerProvider;

    fn inner(&self) -> &Self::Inner {
        &self.inner
    }

    fn chain(&self) -> &Eip155ChainReference {
        &self.chain
    }

    /// Send a meta-transaction with provided `to`, `calldata`, and automatically selected signer.
    ///
    /// This method constructs a transaction from the provided [`MetaTransaction`], automatically
    /// selects the next available signer using round-robin selection, and handles gas pricing
    /// based on whether the network supports EIP-1559.
    ///
    /// If the transaction fails at any point (during submission or receipt fetching), the nonce
    /// for the sending address is reset to force a fresh query on the next transaction. This
    /// ensures correctness even when transactions partially succeed (e.g., submitted but receipt
    /// fetch times out).
    ///
    /// # Gas Pricing Strategy
    ///
    /// - **EIP-1559 networks**: Uses automatic gas pricing via the provider's fillers.
    /// - **Legacy networks**: Fetches the current gas price using `get_gas_price()` and sets it explicitly.
    ///
    /// # Timeout Configuration
    ///
    /// Receipt fetching is subject to a configurable timeout:
    /// - Default: 30 seconds
    /// - Override via `TX_RECEIPT_TIMEOUT_SECS` environment variable
    /// - If the timeout expires, the nonce is reset and an error is returned
    ///
    /// # Parameters
    ///
    /// - `tx`: A [`MetaTransaction`] containing the target address and calldata.
    ///
    /// # Returns
    ///
    /// A [`TransactionReceipt`] once the transaction has been mined and confirmed.
    ///
    /// # Errors
    ///
    /// Returns [`FacilitatorLocalError::ContractCall`] if:
    /// - Gas price fetching fails (on legacy networks)
    /// - Transaction sending fails
    /// - Receipt retrieval fails or times out
    async fn send_transaction(
        &self,
        tx: MetaTransaction,
    ) -> Result<TransactionReceipt, Self::Error> {
        let from_address = self.next_signer_address();
        let mut txr = TransactionRequest::default()
            .with_to(tx.to)
            .with_from(from_address)
            .with_input(tx.calldata);

        if !self.eip1559 {
            let provider = &self.inner;
            let gas: u128 = provider
                .get_gas_price()
                .instrument(tracing::info_span!("get_gas_price"))
                .await?;
            txr.set_gas_price(gas);
        }

        // Estimate gas if not provided
        if txr.gas.is_none() {
            let block_id = if self.flashblocks {
                BlockId::latest()
            } else {
                BlockId::pending()
            };
            let gas_limit = self.inner.estimate_gas(txr.clone()).block(block_id).await?;
            txr.set_gas_limit(gas_limit)
        }

        // Send transaction with error handling for nonce reset
        let pending_tx = match self.inner.send_transaction(txr).await {
            Ok(pending) => pending,
            Err(e) => {
                // Transaction submission failed - reset nonce to force requery
                self.nonce_manager.reset_nonce(from_address).await;
                return Err(MetaTransactionSendError::Transport(e));
            }
        };

        // Get receipt with timeout and error handling for nonce reset
        // Default timeout of 30 seconds is reasonable for most EVM chains
        let timeout = std::time::Duration::from_secs(self.receipt_timeout_secs);

        let watcher = pending_tx
            .with_required_confirmations(tx.confirmations)
            .with_timeout(Some(timeout));

        match watcher.get_receipt().await {
            Ok(receipt) => Ok(receipt),
            Err(e) => {
                // Receipt fetch failed (timeout or other error) - reset nonce to force requery
                self.nonce_manager.reset_nonce(from_address).await;
                Err(MetaTransactionSendError::PendingTransaction(e))
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MetaTransactionSendError {
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error(transparent)]
    PendingTransaction(#[from] PendingTransactionError),
}

impl ChainProviderOps for Eip155ChainProvider {
    fn signer_addresses(&self) -> Vec<String> {
        self.inner
            .signer_addresses()
            .map(|a| a.to_string())
            .collect()
    }

    fn chain_id(&self) -> ChainId {
        self.chain.into()
    }
}

/// Meta-transaction parameters: target address, calldata, and required confirmations.
pub struct MetaTransaction {
    /// Target contract address.
    pub to: Address,
    /// Transaction calldata (encoded function call).
    pub calldata: Bytes,
    /// Number of block confirmations to wait for.
    pub confirmations: u64,
}

/// Trait for sending meta-transactions with custom target and calldata.
pub trait Eip155MetaTransactionProvider {
    /// Error type for operations.
    type Error;
    /// Underlying provider type.
    type Inner: Provider;

    /// Returns reference to underlying provider.
    fn inner(&self) -> &Self::Inner;
    /// Returns reference to chain descriptor.
    fn chain(&self) -> &Eip155ChainReference;

    /// Sends a meta-transaction to the network.
    fn send_transaction(
        &self,
        tx: MetaTransaction,
    ) -> impl Future<Output = Result<TransactionReceipt, Self::Error>> + Send;
}
