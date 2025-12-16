use alloy_primitives::Address;
use alloy_provider::Provider;
use alloy_provider::fillers::NonceManager;
use alloy_transport::TransportResult;
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

/// A nonce manager that caches nonces locally and checks pending transactions on initialization.
///
/// This implementation attempts to improve upon Alloy's `CachedNonceManager` by using `.pending()` when
/// fetching the initial nonce, which includes pending transactions in the mempool. This prevents
/// "nonce too low" errors when the application restarts while transactions are still pending.
///
/// # How it works
///
/// - **First call for an address**: Fetches the nonce using `.pending()`, which includes
///   transactions in the mempool, not just confirmed transactions.
/// - **Subsequent calls**: Increments the cached nonce locally without querying the RPC.
/// - **Per-address tracking**: Each address has its own cached nonce, allowing concurrent
///   transaction submission from multiple addresses.
///
/// # Thread Safety
///
/// The nonce cache is shared across all clones using `Arc<DashMap>`, ensuring that concurrent
/// requests see consistent nonce values. Each address's nonce is protected by its own `Mutex`
/// to prevent race conditions during allocation.
/// ```
#[derive(Clone, Debug, Default)]
pub struct PendingNonceManager {
    /// Cache of nonces per address. Each address has its own mutex-protected nonce value.
    nonces: Arc<DashMap<Address, Arc<Mutex<u64>>>>,
}

#[async_trait]
impl NonceManager for PendingNonceManager {
    async fn get_next_nonce<P, N>(&self, provider: &P, address: Address) -> TransportResult<u64>
    where
        P: Provider<N>,
        N: alloy_network::Network,
    {
        // Use `u64::MAX` as a sentinel value to indicate that the nonce has not been fetched yet.
        const NONE: u64 = u64::MAX;

        // Locks dashmap internally for a short duration to clone the `Arc`.
        // We also don't want to hold the dashmap lock through the await point below.
        let nonce = {
            let rm = self
                .nonces
                .entry(address)
                .or_insert_with(|| Arc::new(Mutex::new(NONE)));
            Arc::clone(rm.value())
        };

        let mut nonce = nonce.lock().await;
        let new_nonce = if *nonce == NONE {
            // Initialize the nonce if we haven't seen this account before.
            tracing::trace!(%address, "fetching nonce");
            provider.get_transaction_count(address).pending().await?
        } else {
            tracing::trace!(%address, current_nonce = *nonce, "incrementing nonce");
            *nonce + 1
        };
        *nonce = new_nonce;
        Ok(new_nonce)
    }
}

impl PendingNonceManager {
    /// Resets the cached nonce for a given address, forcing a fresh query on next use.
    ///
    /// This should be called when a transaction fails, as we cannot be certain of the
    /// actual on-chain state (the transaction may or may not have reached the mempool).
    /// By resetting to the sentinel value, the next call to `get_next_nonce` will query
    /// the RPC provider using `.pending()`, which includes mempool transactions.
    pub async fn reset_nonce(&self, address: Address) {
        if let Some(nonce_lock) = self.nonces.get(&address) {
            let mut nonce = nonce_lock.lock().await;
            *nonce = u64::MAX; // NONE sentinel - will trigger fresh query
            tracing::debug!(%address, "reset nonce cache, will requery on next use");
        }
    }
}
