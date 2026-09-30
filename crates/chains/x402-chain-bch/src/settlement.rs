//! Transaction-claim storage for BCH settlement deduplication.

use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BchSettlementClaim {
    Acquired,
    Same,
    Conflict,
}

#[async_trait]
pub trait BchSettlementStore: Send + Sync {
    async fn claim(&self, txid: &str, binding: &str) -> BchSettlementClaim;
    async fn mark_accepted(&self, txid: &str);
    async fn release(&self, txid: &str);
}

#[derive(Debug, Clone)]
pub struct InMemoryBchSettlementStore {
    records: Arc<Mutex<HashMap<String, (String, bool)>>>,
}

impl Default for InMemoryBchSettlementStore {
    fn default() -> Self {
        Self {
            records: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl BchSettlementStore for InMemoryBchSettlementStore {
    async fn claim(&self, txid: &str, binding: &str) -> BchSettlementClaim {
        let mut records = self.records.lock().await;
        match records.get(txid) {
            None => {
                records.insert(txid.to_owned(), (binding.to_owned(), false));
                BchSettlementClaim::Acquired
            }
            Some((existing, _)) if existing == binding => BchSettlementClaim::Same,
            Some(_) => BchSettlementClaim::Conflict,
        }
    }

    async fn mark_accepted(&self, txid: &str) {
        if let Some((_, accepted)) = self.records.lock().await.get_mut(txid) {
            *accepted = true;
        }
    }

    async fn release(&self, txid: &str) {
        let mut records = self.records.lock().await;
        if records.get(txid).is_some_and(|(_, accepted)| !accepted) {
            records.remove(txid);
        }
    }
}
