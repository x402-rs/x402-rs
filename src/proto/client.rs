use alloy_primitives::U256;
use async_trait::async_trait;

use crate::chain::{ChainId, ChainIdPattern};
use crate::proto;
use crate::scheme::X402SchemeId;

#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct PaymentCandidate {
    pub chain_id: ChainId,
    pub asset: String,
    pub amount: U256,
    pub scheme: String,
    pub x402_version: u8,
    pub pay_to: String,
    pub signer: Box<dyn PaymentCandidateSigner + Send + Sync>,
}

impl PaymentCandidate {
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub async fn sign(&self) -> Result<String, X402Error> {
        self.signer.sign_payment().await
    }
}

#[async_trait::async_trait]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub trait X402SchemeClient: X402SchemeId + Send + Sync {
    fn accept(&self, payment_required: &proto::PaymentRequired) -> Vec<PaymentCandidate>;
}

#[async_trait]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub trait PaymentCandidateSigner {
    async fn sign_payment(&self) -> Result<String, X402Error>;
}

#[derive(Debug, thiserror::Error)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub enum X402Error {
    #[error("No matching payment option found")]
    NoMatchingPaymentOption,

    #[error("Request is not cloneable (streaming body?)")]
    RequestNotCloneable,

    #[error("Failed to parse 402 response: {0}")]
    ParseError(String),

    #[error("Failed to sign payment: {0}")]
    SigningError(String),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

#[allow(dead_code)] // Public for consumption by downstream crates.
pub enum HttpTransport<A> {
    V1(A),
    V2(A),
}

// ============================================================================
// PaymentSelector - Selection strategy
// ============================================================================

/// Trait for selecting the best payment candidate from available options.
#[allow(dead_code)] // Public for consumption by downstream crates.
pub trait PaymentSelector: Send + Sync {
    fn select<'a>(&self, candidates: &'a [PaymentCandidate]) -> Option<&'a PaymentCandidate>;
}

/// Default selector: returns the first matching candidate.
/// Order is determined by registration order of scheme clients.
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct FirstMatch;

impl PaymentSelector for FirstMatch {
    fn select<'a>(&self, candidates: &'a [PaymentCandidate]) -> Option<&'a PaymentCandidate> {
        candidates.first()
    }
}

/// Selector that prefers chains matching patterns in priority order.
/// The first pattern in the vector has the highest priority, the last has the lowest.
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct PreferChain(Vec<ChainIdPattern>);

#[allow(dead_code)] // Public for consumption by downstream crates.
impl PreferChain {
    pub fn new<P: Into<Vec<ChainIdPattern>>>(patterns: P) -> Self {
        Self(patterns.into())
    }

    pub fn or_chain<P: Into<Vec<ChainIdPattern>>>(self, patterns: P) -> PreferChain {
        PreferChain(self.0.into_iter().chain(patterns.into()).collect())
    }
}

impl PaymentSelector for PreferChain {
    fn select<'a>(&self, candidates: &'a [PaymentCandidate]) -> Option<&'a PaymentCandidate> {
        // Try each pattern in priority order
        for pattern in &self.0 {
            if let Some(candidate) = candidates.iter().find(|c| pattern.matches(&c.chain_id)) {
                return Some(candidate);
            }
        }
        // Fall back to first match if no patterns matched
        candidates.first()
    }
}

/// Selector that only accepts payments up to a maximum amount.
#[allow(dead_code)]
pub struct MaxAmount(pub U256);

impl PaymentSelector for MaxAmount {
    fn select<'a>(&self, candidates: &'a [PaymentCandidate]) -> Option<&'a PaymentCandidate> {
        candidates.iter().find(|c| c.amount <= self.0)
    }
}
