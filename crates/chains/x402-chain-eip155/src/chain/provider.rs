use alloy_network::{Ethereum as AlloyEthereum, EthereumWallet, NetworkWallet, TransactionBuilder};
use alloy_primitives::{Address, B256, Bytes, TxHash};
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
use std::num::{NonZeroU64, NonZeroUsize};
use std::ops::Deref;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;
use tower::ServiceBuilder;
use x402_types::chain::{ChainId, ChainProviderOps, FromConfig};

#[cfg(feature = "telemetry")]
use tracing::Instrument;

use crate::chain::authorization_use::{
    AUTHORIZATION_USE_LOOKBACK, AuthorizationUse, Eip3009Authorization, query_authorization_use,
};
use crate::chain::config::{Eip155ChainConfig, RpcConfig};
use crate::chain::pending_nonce_manager::PendingNonceManager;
use crate::chain::pending_settlement::{InMemoryPendingSettlementStore, PendingSettlementStore};
use crate::chain::permit2::{EXACT_PERMIT2_PROXY_ADDRESS, PERMIT2_ADDRESS};
use crate::chain::types::Eip155ChainReference;
use crate::v1_eip155_exact::VALIDATOR_ADDRESS;

/// Combined filler type for gas, blob gas, nonce, and chain ID.
pub type InnerFiller = JoinFill<
    GasFiller,
    JoinFill<BlobGasFiller, JoinFill<NonceFiller<PendingNonceManager>, ChainIdFiller>>,
>;

/// How many more times [`Eip155ChainProvider::send_transaction`] looks a receipt up after
/// the receipt watcher fails for a transaction that is already broadcast.
const RECEIPT_RETRY_ATTEMPTS: u32 = 3;

/// Wait before the first of those lookups. Each next wait is twice as long.
const RECEIPT_RETRY_INITIAL_DELAY: Duration = Duration::from_secs(1);

/// Wait between receipt lookups when a retried settlement is reconciled against the
/// transaction it already broadcast.
const RECONCILE_POLL_INTERVAL: Duration = Duration::from_secs(1);

static REQUIRED_CONTRACT_ADDRESSES: LazyLock<Vec<Address>> = LazyLock::new(|| {
    vec![
        VALIDATOR_ADDRESS,
        PERMIT2_ADDRESS,
        EXACT_PERMIT2_PROXY_ADDRESS,
    ]
});

/// The fully composed Ethereum provider type used in this project.
///
/// Combines multiple filler layers for gas, nonce, chain ID, blob gas, and wallet signing,
/// and wraps a [`RootProvider`] for actual JSON-RPC communication.
pub type InnerProvider = FillProvider<
    JoinFill<JoinFill<Identity, InnerFiller>, WalletFiller<EthereumWallet>>,
    RootProvider,
>;

/// Provider for interacting with EVM-compatible blockchains.
///
/// This provider handles:
/// - Transaction signing with multiple signers (round-robin selection)
/// - Nonce management with automatic reset on failures
/// - Gas estimation and pricing (EIP-1559 and legacy)
/// - Transaction receipt fetching with configurable timeouts
///
/// # Multiple Signers
///
/// The provider supports multiple signers for load distribution. When sending
/// transactions, signers are selected in round-robin fashion to distribute
/// the transaction load and avoid nonce conflicts.
///
/// # Nonce Management
///
/// Uses [`PendingNonceManager`] to track nonces locally and query pending
/// transactions on initialization. If a transaction fails, the nonce is
/// automatically reset to force a fresh query on the next transaction.
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
    /// Settlements broadcast without a known outcome, for the resource server's retry.
    pending_settlements: Arc<InMemoryPendingSettlementStore>,
    /// Whether [`Eip155MetaTransactionProvider::find_authorization_use`] looks on chain.
    reconcile_used_authorizations: bool,
    /// Most blocks one `eth_getLogs` call of that lookup covers.
    log_query_max_blocks: NonZeroU64,
}

impl Eip155ChainProvider {
    #[allow(unused_variables)] // chain_id is needed for tracing only here
    pub fn rpc_client(chain_id: ChainId, rpc: &[RpcConfig]) -> RpcClient {
        let transports = rpc
            .iter()
            .filter_map(|provider_config| {
                let scheme = provider_config.http.scheme();
                let is_http = scheme == "http" || scheme == "https";
                if !is_http {
                    return None;
                }
                let rpc_url = provider_config.http.deref().clone();
                #[cfg(feature = "telemetry")]
                tracing::info!(chain=%chain_id, rpc_url=%rpc_url, rate_limit=?provider_config.rate_limit, "Using HTTP transport");
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
        RpcClient::new(fallback, false)
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

    /// Fetches the receipt of `tx_hash`, counting it only once its block has
    /// `confirmations` confirmations.
    async fn confirmed_receipt(
        &self,
        tx_hash: TxHash,
        confirmations: u64,
    ) -> Result<Option<TransactionReceipt>, TransportError> {
        let Some(receipt) = self.inner.get_transaction_receipt(tx_hash).await? else {
            return Ok(None);
        };
        if confirmations > 1 {
            let mined_in = receipt.block_number.unwrap_or(u64::MAX);
            let head = self.inner.get_block_number().await?;
            if head.saturating_add(1) < mined_in.saturating_add(confirmations) {
                return Ok(None);
            }
        }
        Ok(Some(receipt))
    }
}

/// Runs `lookup` up to `attempts` times and returns the first receipt it finds.
///
/// Sleeps `initial_delay` before the first run and twice as long before each next one.
/// A lookup that fails counts the same as one that finds nothing.
async fn retry_receipt_lookup<R, E, F, Fut>(
    attempts: u32,
    initial_delay: Duration,
    mut lookup: F,
) -> Option<R>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<R>, E>>,
{
    let mut delay = initial_delay;
    for _ in 0..attempts {
        tokio::time::sleep(delay).await;
        if let Ok(Some(receipt)) = lookup().await {
            return Some(receipt);
        }
        delay = delay.saturating_mul(2);
    }
    None
}

/// Runs `lookup` every `interval` until it finds a receipt or `timeout` has passed since the
/// start, and returns that receipt. The first lookup runs at once. A lookup that fails counts
/// the same as one that finds nothing.
async fn poll_receipt_lookup<R, E, F, Fut>(
    timeout: Duration,
    interval: Duration,
    mut lookup: F,
) -> Option<R>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Option<R>, E>>,
{
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        if let Ok(Some(receipt)) = lookup().await {
            return Some(receipt);
        }
        if tokio::time::Instant::now() + interval > deadline {
            return None;
        }
        tokio::time::sleep(interval).await;
    }
}

/// Creates a new provider from configuration.
///
/// Initializes signers, RPC transports, and the nonce manager.
///
/// # Errors
///
/// Returns an error if:
/// - No signers are configured
/// - Signer private keys are invalid
/// - RPC transport initialization fails
#[async_trait::async_trait]
impl FromConfig<Eip155ChainConfig> for Eip155ChainProvider {
    async fn from_config(config: &Eip155ChainConfig) -> Result<Self, Box<dyn std::error::Error>> {
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
        let client = Self::rpc_client(config.chain_id(), config.rpc());

        // 3. Provider
        // Create nonce manager explicitly so we can store a reference for error handling
        let nonce_manager = PendingNonceManager::default();
        // Build the filler stack: Gas -> BlobGas -> Nonce -> ChainId
        // This mirrors the InnerFiller type but with our custom nonce manager
        let filler = JoinFill::new(
            GasFiller::default(),
            JoinFill::new(
                BlobGasFiller::default(),
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

        assert_contracts_exists(&inner).await?;

        #[cfg(feature = "telemetry")]
        tracing::info!(chain=%config.chain_id(), signers=?signer_addresses, "Using EVM provider");

        Ok(Self {
            chain: config.chain_reference(),
            eip1559: config.eip1559(),
            flashblocks: config.flashblocks(),
            receipt_timeout_secs: config.receipt_timeout_secs(),
            inner,
            signer_addresses,
            signer_cursor,
            nonce_manager,
            pending_settlements: Arc::new(InMemoryPendingSettlementStore::default()),
            reconcile_used_authorizations: config.reconcile_used_authorizations(),
            log_query_max_blocks: config.log_query_max_blocks(),
        })
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
    /// Once the transaction is broadcast, a failed receipt fetch does not mean it failed: the
    /// receipt is looked up again a few times, and if it is still unknown the error is
    /// [`MetaTransactionSendError::ReceiptUnavailable`], which keeps the transaction hash.
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
    /// Returns [`MetaTransactionSendError::Transport`] if gas price fetching (on legacy
    /// networks), gas estimation or the submission fails, so no transaction hash came back.
    /// Returns [`MetaTransactionSendError::ReceiptUnavailable`] if the submission returned a
    /// hash but the receipt could not be fetched or did not arrive before the timeout.
    async fn send_transaction(
        &self,
        tx: MetaTransaction,
    ) -> Result<TransactionReceipt, Self::Error> {
        let from_address = tx.from.unwrap_or_else(|| self.next_signer_address());
        let mut txr = TransactionRequest::default()
            .with_to(tx.to)
            .with_from(from_address)
            .with_input(tx.calldata);

        if !self.eip1559 {
            let provider = &self.inner;
            let gas_fut = provider.get_gas_price();
            #[cfg(feature = "telemetry")]
            let gas: u128 = gas_fut
                .instrument(tracing::info_span!("get_gas_price"))
                .await?;
            #[cfg(not(feature = "telemetry"))]
            let gas: u128 = gas_fut.await?;
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
        // Broadcast from here on: keep the hash, whatever happens to the receipt.
        let tx_hash = *pending_tx.tx_hash();
        let confirmations = tx.confirmations;

        // Get receipt with timeout and error handling for nonce reset
        // Default timeout of 30 seconds is reasonable for most EVM chains
        let timeout = Duration::from_secs(self.receipt_timeout_secs);

        let watcher = pending_tx
            .with_required_confirmations(confirmations)
            .with_timeout(Some(timeout));

        match watcher.get_receipt().await {
            Ok(receipt) => Ok(receipt),
            Err(e) => {
                // The watcher stops at the first failed RPC call, but the transaction may be
                // mined anyway: look the receipt up directly before giving up on it.
                #[cfg(feature = "telemetry")]
                tracing::warn!(tx = %tx_hash, "Receipt fetch failed after broadcast, retrying");
                let receipt = retry_receipt_lookup(
                    RECEIPT_RETRY_ATTEMPTS,
                    RECEIPT_RETRY_INITIAL_DELAY,
                    || self.confirmed_receipt(tx_hash, confirmations),
                )
                .await;
                match receipt {
                    Some(receipt) => Ok(receipt),
                    None => {
                        // Receipt still unknown - reset nonce to force requery
                        self.nonce_manager.reset_nonce(from_address).await;
                        Err(MetaTransactionSendError::ReceiptUnavailable { tx_hash, source: e })
                    }
                }
            }
        }
    }

    fn pending_settlements(&self) -> Option<&dyn PendingSettlementStore> {
        Some(self.pending_settlements.as_ref())
    }

    /// Looks the receipt of `tx_hash` up every second, for at most the configured receipt
    /// timeout. Failed lookups count as "not yet".
    async fn find_receipt(&self, tx_hash: TxHash) -> Option<TransactionReceipt> {
        poll_receipt_lookup(
            Duration::from_secs(self.receipt_timeout_secs),
            RECONCILE_POLL_INTERVAL,
            || self.confirmed_receipt(tx_hash, 1),
        )
        .await
    }

    /// Looks the use of `authorization` up on chain when `reconcile_used_authorizations` is
    /// set in the chain's configuration, within [`AUTHORIZATION_USE_LOOKBACK`] and with at most
    /// `log_query_max_blocks` blocks per `eth_getLogs` call, then waits for the receipt of the
    /// transaction found as [`Self::find_receipt`] does. A failed lookup counts as not found.
    async fn find_authorization_use(
        &self,
        authorization: Eip3009Authorization,
    ) -> Option<AuthorizationUse> {
        if !self.reconcile_used_authorizations {
            return None;
        }
        let tx_hash = match query_authorization_use(
            &self.inner,
            &authorization,
            AUTHORIZATION_USE_LOOKBACK,
            self.log_query_max_blocks,
        )
        .await
        {
            Ok(tx_hash) => tx_hash?,
            Err(error) => {
                #[cfg(feature = "telemetry")]
                tracing::warn!(%error, nonce = %authorization.nonce, "Lookup of a used authorization failed");
                let _ = error;
                return None;
            }
        };
        Some(AuthorizationUse {
            tx_hash,
            receipt: self.find_receipt(tx_hash).await,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MetaTransactionSendError {
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error(transparent)]
    PendingTransaction(#[from] PendingTransactionError),
    /// The transaction was broadcast, but its receipt could not be fetched.
    ///
    /// It may still be mined, so the hash is kept for the caller to reconcile with.
    #[error("Transaction {tx_hash} was broadcast, but its receipt could not be fetched: {source}")]
    ReceiptUnavailable {
        /// Hash of the broadcast transaction.
        tx_hash: TxHash,
        /// Why the receipt could not be fetched.
        #[source]
        source: PendingTransactionError,
    },
    #[allow(dead_code)] // Public for consumption by downstream crates.
    #[error("{0}")]
    Custom(String),
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

/// Provides access to the EIP-155 signer addresses held by a facilitator provider.
///
/// Implementations return the set of addresses whose private keys the provider
/// controls and can use to submit on-chain transactions. The facilitator exposes
/// one of these addresses to clients via the `supported()` endpoint so they can
/// embed it in the Permit2 witness, ensuring only this facilitator can settle the
/// authorized payment.
pub trait Eip155SignerAddresses {
    /// Returns an iterator over the signer addresses available on this provider.
    fn signer_addresses(&self) -> Vec<Address>;
}

impl<T> Eip155SignerAddresses for Arc<T>
where
    T: Eip155SignerAddresses,
{
    fn signer_addresses(&self) -> Vec<Address> {
        (**self).signer_addresses()
    }
}

impl Eip155SignerAddresses for Eip155ChainProvider {
    fn signer_addresses(&self) -> Vec<Address> {
        (*self.signer_addresses).clone()
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
    /// Optional sender address.
    pub from: Option<Address>,
}

impl MetaTransaction {
    pub fn new(to: Address, calldata: Bytes) -> Self {
        Self {
            to,
            calldata,
            confirmations: 1,
            from: None,
        }
    }

    pub fn with_from(mut self, from: Address) -> Self {
        self.from = Some(from);
        self
    }
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

    /// The store of settlements that were broadcast but whose receipt could not be fetched,
    /// which lets a retried `/settle` reconcile against the transaction already sent.
    /// `None` (the default) keeps no store: a retry is verified and settled from scratch.
    fn pending_settlements(&self) -> Option<&dyn PendingSettlementStore> {
        None
    }

    /// Looks up the receipt of a transaction that was already broadcast, waiting for it for a
    /// bounded time. `None` if it was not found in that time. The default looks nothing up.
    fn find_receipt(
        &self,
        tx_hash: TxHash,
    ) -> impl Future<Output = Option<TransactionReceipt>> + Send {
        let _ = tx_hash;
        std::future::ready(None)
    }

    /// Looks up the transaction that used an ERC-3009 authorization, and its receipt, for a
    /// settle request that has no pending-settlement entry and failed to settle: if the
    /// authorization was already used, that transaction may be the request's settlement. `None`
    /// if it was not found. The default looks nothing up, so such a request fails as before.
    fn find_authorization_use(
        &self,
        authorization: Eip3009Authorization,
    ) -> impl Future<Output = Option<AuthorizationUse>> + Send {
        let _ = authorization;
        std::future::ready(None)
    }
}

impl<T: Eip155MetaTransactionProvider> Eip155MetaTransactionProvider for Arc<T> {
    type Error = T::Error;
    type Inner = T::Inner;

    fn inner(&self) -> &Self::Inner {
        (**self).inner()
    }

    fn chain(&self) -> &Eip155ChainReference {
        (**self).chain()
    }

    fn send_transaction(
        &self,
        tx: MetaTransaction,
    ) -> impl Future<Output = Result<TransactionReceipt, Self::Error>> + Send {
        (**self).send_transaction(tx)
    }

    fn pending_settlements(&self) -> Option<&dyn PendingSettlementStore> {
        (**self).pending_settlements()
    }

    fn find_receipt(
        &self,
        tx_hash: TxHash,
    ) -> impl Future<Output = Option<TransactionReceipt>> + Send {
        (**self).find_receipt(tx_hash)
    }

    fn find_authorization_use(
        &self,
        authorization: Eip3009Authorization,
    ) -> impl Future<Output = Option<AuthorizationUse>> + Send {
        (**self).find_authorization_use(authorization)
    }
}

pub async fn assert_contracts_exists<P: Provider>(
    provider: &P,
) -> Result<(), Box<dyn std::error::Error>> {
    for address in REQUIRED_CONTRACT_ADDRESSES.deref() {
        let code = provider.get_code_at(*address).await?;
        if code.is_empty() {
            return Err(
                format!("Contract at address {address} does not exist (empty code)").into(),
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[tokio::test(start_paused = true)]
    async fn test_retry_receipt_lookup_finds_receipt_after_failed_lookups() {
        let calls = AtomicU32::new(0);
        let receipt = retry_receipt_lookup(3, Duration::from_secs(1), || {
            let call = calls.fetch_add(1, Ordering::Relaxed);
            async move {
                match call {
                    0 => Err("HTTP error 503"),
                    1 => Ok(None),
                    _ => Ok(Some("receipt")),
                }
            }
        })
        .await;
        assert_eq!(receipt, Some("receipt"));
        assert_eq!(calls.load(Ordering::Relaxed), 3);
    }

    #[tokio::test(start_paused = true)]
    async fn test_retry_receipt_lookup_stops_at_first_receipt() {
        let calls = AtomicU32::new(0);
        let receipt = retry_receipt_lookup(3, Duration::from_secs(1), || {
            calls.fetch_add(1, Ordering::Relaxed);
            async { Ok::<_, ()>(Some("receipt")) }
        })
        .await;
        assert_eq!(receipt, Some("receipt"));
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn test_retry_receipt_lookup_gives_up_after_all_attempts() {
        let calls = AtomicU32::new(0);
        let start = tokio::time::Instant::now();
        let receipt = retry_receipt_lookup(3, Duration::from_secs(1), || {
            calls.fetch_add(1, Ordering::Relaxed);
            async { Err::<Option<()>, _>("HTTP error 503") }
        })
        .await;
        assert_eq!(receipt, None);
        assert_eq!(calls.load(Ordering::Relaxed), 3);
        // Waits 1 s, 2 s and 4 s before the three lookups.
        assert_eq!(start.elapsed(), Duration::from_secs(7));
    }

    #[tokio::test(start_paused = true)]
    async fn test_poll_receipt_lookup_finds_receipt_once_lookups_recover() {
        let calls = AtomicU32::new(0);
        let start = tokio::time::Instant::now();
        let receipt = poll_receipt_lookup(Duration::from_secs(30), Duration::from_secs(1), || {
            let call = calls.fetch_add(1, Ordering::Relaxed);
            async move {
                if call < 5 {
                    Err("HTTP error 503")
                } else {
                    Ok(Some("receipt"))
                }
            }
        })
        .await;
        assert_eq!(receipt, Some("receipt"));
        assert_eq!(calls.load(Ordering::Relaxed), 6);
        // The first lookup runs at once, then one every second.
        assert_eq!(start.elapsed(), Duration::from_secs(5));
    }

    #[tokio::test(start_paused = true)]
    async fn test_poll_receipt_lookup_gives_up_at_the_timeout() {
        let calls = AtomicU32::new(0);
        let start = tokio::time::Instant::now();
        let receipt = poll_receipt_lookup(Duration::from_secs(10), Duration::from_secs(1), || {
            calls.fetch_add(1, Ordering::Relaxed);
            async { Ok::<Option<()>, ()>(None) }
        })
        .await;
        assert_eq!(receipt, None);
        assert_eq!(calls.load(Ordering::Relaxed), 11);
        assert_eq!(start.elapsed(), Duration::from_secs(10));
    }

    mod authorization_use {
        use super::*;
        use crate::chain::authorization_use::LOG_QUERY_MAX_BLOCKS;
        use crate::chain::authorization_use::tests::{
            ASSET, PAYER, TX, abi_bool, authorization, authorization_used_log, block,
        };
        use alloy_primitives::b256;
        use alloy_transport::mock::Asserter;

        /// A provider on a mock RPC that answers from `asserter`, in order.
        fn provider(
            asserter: &Asserter,
            reconcile_used_authorizations: bool,
        ) -> Eip155ChainProvider {
            provider_with_log_range(
                asserter,
                reconcile_used_authorizations,
                LOG_QUERY_MAX_BLOCKS,
            )
        }

        fn provider_with_log_range(
            asserter: &Asserter,
            reconcile_used_authorizations: bool,
            log_query_max_blocks: NonZeroU64,
        ) -> Eip155ChainProvider {
            let signer = PrivateKeySigner::from_bytes(&b256!(
                "0xcafe000000000000000000000000000000000000000000000000000000000001"
            ))
            .unwrap();
            let signer_address = signer.address();
            let nonce_manager = PendingNonceManager::default();
            let filler = JoinFill::new(
                GasFiller::default(),
                JoinFill::new(
                    BlobGasFiller::default(),
                    JoinFill::new(
                        NonceFiller::new(nonce_manager.clone()),
                        ChainIdFiller::default(),
                    ),
                ),
            );
            let inner: InnerProvider = ProviderBuilder::default()
                .filler(filler)
                .wallet(EthereumWallet::from(signer))
                .connect_client(RpcClient::mocked(asserter.clone()));
            Eip155ChainProvider {
                chain: Eip155ChainReference::new(84532),
                eip1559: true,
                flashblocks: false,
                receipt_timeout_secs: 3,
                inner,
                signer_addresses: Arc::new(vec![signer_address]),
                signer_cursor: Arc::new(AtomicUsize::new(0)),
                nonce_manager,
                pending_settlements: Arc::new(InMemoryPendingSettlementStore::default()),
                reconcile_used_authorizations,
                log_query_max_blocks,
            }
        }

        fn receipt_of(tx_hash: TxHash) -> serde_json::Value {
            serde_json::json!({
                "type": "0x2",
                "status": "0x1",
                "cumulativeGasUsed": "0x1",
                "logs": [authorization_used_log(100)],
                "logsBloom": format!("0x{}", "0".repeat(512)),
                "transactionHash": tx_hash,
                "transactionIndex": "0x0",
                "blockHash": B256::with_last_byte(2),
                "blockNumber": "0x64",
                "gasUsed": "0x1",
                "effectiveGasPrice": "0x1",
                "from": Address::ZERO,
                "to": ASSET,
                "contractAddress": null
            })
        }

        /// The answers for an authorization used in block 100, the latest block, which is
        /// also where the range starts since the authorization became valid at its time.
        fn push_used_in_latest_block(asserter: &Asserter) {
            asserter.push_success(&abi_bool(true));
            asserter.push_success(&block(100, 1_000));
            asserter.push_success(&vec![authorization_used_log(100)]);
            asserter.push_success(&receipt_of(TX));
        }

        #[tokio::test]
        async fn test_lookup_is_off_unless_configured() {
            let asserter = Asserter::new();
            push_used_in_latest_block(&asserter);
            let found = provider(&asserter, false)
                .find_authorization_use(authorization(1_000, 2_000))
                .await;
            assert!(found.is_none());
            assert_eq!(asserter.read_q().len(), 4, "no RPC call was made");
        }

        #[tokio::test]
        async fn test_lookup_finds_the_transaction_and_its_receipt() {
            let asserter = Asserter::new();
            push_used_in_latest_block(&asserter);
            let found = provider(&asserter, true)
                .find_authorization_use(authorization(1_000, 2_000))
                .await
                .expect("the transaction that used the authorization");
            assert_eq!(found.tx_hash, TX);
            let receipt = found.receipt.expect("its receipt");
            assert_eq!(receipt.transaction_hash, TX);
            assert_eq!(receipt.logs()[0].topics()[1], PAYER.into_word());
            assert!(asserter.read_q().is_empty());
        }

        #[tokio::test]
        async fn test_lookup_queries_logs_in_ranges_of_the_configured_size() {
            let asserter = Asserter::new();
            asserter.push_success(&abi_bool(true));
            asserter.push_success(&block(100, 1_000));
            // The authorization became valid at time 996: the range is 84..=100, 17 blocks.
            asserter.push_success(&block(84, 968));
            // With 10 blocks per call, 91..=100 has nothing and 84..=90 has the use; with the
            // default 100, one call over 84..=100 would get the first answer and stop there.
            asserter.push_success(&Vec::<serde_json::Value>::new());
            asserter.push_success(&vec![authorization_used_log(85)]);
            asserter.push_success(&receipt_of(TX));
            let found = provider_with_log_range(&asserter, true, NonZeroU64::new(10).unwrap())
                .find_authorization_use(authorization(996, 2_000))
                .await
                .expect("the transaction that used the authorization");
            assert_eq!(found.tx_hash, TX);
            assert!(found.receipt.is_some());
            assert!(asserter.read_q().is_empty());
        }

        #[tokio::test(start_paused = true)]
        async fn test_transaction_found_without_its_receipt_comes_back_with_its_hash() {
            let asserter = Asserter::new();
            asserter.push_success(&abi_bool(true));
            asserter.push_success(&block(100, 1_000));
            asserter.push_success(&vec![authorization_used_log(100)]);
            // Every receipt lookup in the wait (receipt_timeout_secs, 3 s) finds nothing.
            for _ in 0..4 {
                asserter.push_success(&serde_json::Value::Null);
            }
            let found = provider(&asserter, true)
                .find_authorization_use(authorization(1_000, 2_000))
                .await
                .expect("the transaction that used the authorization");
            assert_eq!(found.tx_hash, TX);
            assert!(found.receipt.is_none());
            assert!(
                asserter.read_q().is_empty(),
                "looked the receipt up 4 times"
            );
        }

        #[tokio::test(start_paused = true)]
        async fn test_failed_lookup_counts_as_not_found() {
            let asserter = Asserter::new();
            asserter.push_success(&abi_bool(true));
            asserter.push_failure_msg("no backend is currently healthy to serve traffic");
            let found = provider(&asserter, true)
                .find_authorization_use(authorization(1_000, 2_000))
                .await;
            assert!(found.is_none());
        }
    }
}
