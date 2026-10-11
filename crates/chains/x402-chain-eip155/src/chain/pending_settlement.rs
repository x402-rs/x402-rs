//! Settlements that were broadcast but whose outcome is not known yet.
//!
//! When the receipt of a settlement transaction cannot be fetched, `/settle` answers
//! `settlement_pending` with the transaction hash. A resource server on the reference x402 SDK
//! then retries `/settle` once with the same payload. Without a record of the broadcast, that
//! retry is verified from scratch and fails once the first transaction has landed (the
//! authorization is already used), so the resource server ends on a failure for a payment that
//! was made. A [`PendingSettlementStore`] keeps the hash per payment, so the retry is reconciled
//! against the transaction already sent instead of being verified and sent again.

use alloy_primitives::{B256, TxHash, keccak256};
use dashmap::DashMap;
use serde::Serialize;
use std::fmt::Debug;
use std::time::Duration;
use tokio::time::Instant;

/// How long an entry is kept, as in the reference implementation (`PENDING_SETTLEMENT_TTL_MS`).
pub const PENDING_SETTLEMENT_TTL: Duration = Duration::from_secs(5 * 60);

/// A settlement transaction that was broadcast for a payment whose outcome is not known yet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PendingSettlement {
    /// Hash of the broadcast transaction.
    pub tx_hash: TxHash,
    /// Hash of the settle request (payment payload and requirements) the transaction was
    /// broadcast for. A later request is reconciled against the entry only if it is the same
    /// request, so a signature re-sent with other requirements cannot claim the settlement.
    pub request_hash: B256,
}

/// Records settlements that were broadcast but whose receipt could not be fetched, keyed by the
/// payment (see [`pending_settlement_key`]).
///
/// The default, [`InMemoryPendingSettlementStore`], lives in the facilitator process, so a retry
/// is reconciled only when it reaches the same process. Facilitators that run several instances
/// without session affinity can supply a shared store. Implementations must be safe for
/// concurrent use.
// async_trait marks the boxed futures it returns `#[must_use]` a second time.
#[allow(clippy::double_must_use)]
#[async_trait::async_trait]
pub trait PendingSettlementStore: Debug + Send + Sync {
    /// Records `settlement` for the payment `key`, replacing any earlier entry.
    async fn insert(&self, key: B256, settlement: PendingSettlement);

    /// Removes and returns the transaction recorded for `key`, when it was recorded for the same
    /// `request_hash` and has not expired. An entry for another request is left in place.
    async fn take(&self, key: &B256, request_hash: &B256) -> Option<TxHash>;

    /// Removes any entry for `key`.
    async fn remove(&self, key: &B256);
}

/// The default [`PendingSettlementStore`]: a concurrent map in the facilitator process, with
/// entries dropped [`PENDING_SETTLEMENT_TTL`] after they were recorded.
#[derive(Debug)]
pub struct InMemoryPendingSettlementStore {
    entries: DashMap<B256, (PendingSettlement, Instant)>,
    ttl: Duration,
}

impl InMemoryPendingSettlementStore {
    /// Creates an empty store whose entries expire after `ttl`.
    pub fn new(ttl: Duration) -> Self {
        Self {
            entries: DashMap::new(),
            ttl,
        }
    }

    /// Number of entries currently held, expired ones included until the next access prunes them.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the store holds no entry.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn prune(&self) {
        let ttl = self.ttl;
        self.entries
            .retain(|_, (_, recorded_at)| recorded_at.elapsed() < ttl);
    }
}

impl Default for InMemoryPendingSettlementStore {
    fn default() -> Self {
        Self::new(PENDING_SETTLEMENT_TTL)
    }
}

#[async_trait::async_trait]
impl PendingSettlementStore for InMemoryPendingSettlementStore {
    async fn insert(&self, key: B256, settlement: PendingSettlement) {
        self.prune();
        self.entries.insert(key, (settlement, Instant::now()));
    }

    async fn take(&self, key: &B256, request_hash: &B256) -> Option<TxHash> {
        self.prune();
        let ttl = self.ttl;
        self.entries
            .remove_if(key, |_, (settlement, recorded_at)| {
                &settlement.request_hash == request_hash && recorded_at.elapsed() < ttl
            })
            .map(|(_, (settlement, _))| settlement.tx_hash)
    }

    async fn remove(&self, key: &B256) {
        self.entries.remove(key);
    }
}

/// Key of a payment in a [`PendingSettlementStore`]: the hash of its signature, which no other
/// payment shares.
pub fn pending_settlement_key(signature: &[u8]) -> B256 {
    keccak256(signature)
}

/// Hash of a settle request: the payment payload and the requirements, as serialized JSON.
///
/// Serialization of the typed request is deterministic, so the same request re-sent gives the
/// same hash whatever the whitespace of the original body. `None` if the request does not
/// serialize, in which case nothing is recorded for it.
pub fn settle_request_hash<T: Serialize, R: Serialize>(
    payload: &T,
    requirements: &R,
) -> Option<B256> {
    serde_json::to_vec(&(payload, requirements))
        .ok()
        .map(keccak256)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::b256;

    const KEY: B256 = b256!("0x1111111111111111111111111111111111111111111111111111111111111111");
    const TX: TxHash = b256!("0xa5f6a7027df46b5da388406fbc39e468f2466968f698ffbd21ad0e674af58601");
    const REQUEST: B256 =
        b256!("0x2222222222222222222222222222222222222222222222222222222222222222");
    const OTHER_REQUEST: B256 =
        b256!("0x3333333333333333333333333333333333333333333333333333333333333333");

    fn pending() -> PendingSettlement {
        PendingSettlement {
            tx_hash: TX,
            request_hash: REQUEST,
        }
    }

    #[tokio::test(start_paused = true)]
    async fn test_take_returns_the_hash_once() {
        let store = InMemoryPendingSettlementStore::default();
        store.insert(KEY, pending()).await;
        assert_eq!(store.take(&KEY, &REQUEST).await, Some(TX));
        assert_eq!(store.take(&KEY, &REQUEST).await, None);
        assert!(store.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn test_take_with_another_request_leaves_the_entry() {
        let store = InMemoryPendingSettlementStore::default();
        store.insert(KEY, pending()).await;
        assert_eq!(store.take(&KEY, &OTHER_REQUEST).await, None);
        assert_eq!(store.take(&KEY, &REQUEST).await, Some(TX));
    }

    #[tokio::test(start_paused = true)]
    async fn test_entries_expire_after_the_ttl() {
        let store = InMemoryPendingSettlementStore::new(Duration::from_secs(60));
        store.insert(KEY, pending()).await;
        tokio::time::advance(Duration::from_secs(59)).await;
        assert_eq!(store.len(), 1);
        tokio::time::advance(Duration::from_secs(2)).await;
        assert_eq!(store.take(&KEY, &REQUEST).await, None);
        assert!(store.is_empty());
    }

    #[tokio::test(start_paused = true)]
    async fn test_remove_drops_the_entry() {
        let store = InMemoryPendingSettlementStore::default();
        store.insert(KEY, pending()).await;
        store.remove(&KEY).await;
        assert_eq!(store.take(&KEY, &REQUEST).await, None);
    }

    #[test]
    fn test_request_hash_depends_on_payload_and_requirements() {
        let a = settle_request_hash(&("sig", 1), &"pay-to-a").unwrap();
        let b = settle_request_hash(&("sig", 1), &"pay-to-b").unwrap();
        assert_eq!(a, settle_request_hash(&("sig", 1), &"pay-to-a").unwrap());
        assert_ne!(a, b);
    }

    #[test]
    fn test_key_is_the_signature_hash() {
        assert_eq!(pending_settlement_key(b"sig"), keccak256(b"sig"));
        assert_ne!(
            pending_settlement_key(b"sig"),
            pending_settlement_key(b"other")
        );
    }
}
