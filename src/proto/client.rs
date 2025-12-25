use alloy_primitives::U256;

use crate::chain::{ChainId, ChainIdPattern};

/// Trait for types that can be used as payment candidates in selection.
/// This allows the selector to work with any type that provides the necessary
/// payment information, not just the concrete PaymentCandidate type.
pub trait PaymentCandidateLike {
    /// Get the chain ID for this payment candidate
    fn chain_id(&self) -> &ChainId;

    /// Get the asset address for this payment candidate
    fn asset(&self) -> &str;

    /// Get the payment amount for this payment candidate
    fn amount(&self) -> U256;

    /// Get the scheme name for this payment candidate
    fn scheme(&self) -> &str;

    /// Get the x402 protocol version for this payment candidate
    fn x402_version(&self) -> u8;
}

// ============================================================================
// PaymentSelector - Selection strategy
// ============================================================================

/// Trait for selecting the best payment candidate from available options.
pub trait PaymentSelector: Send + Sync {
    fn select<'a, T: PaymentCandidateLike>(&self, candidates: &'a [T]) -> Option<&'a T>;
}

/// Default selector: returns the first matching candidate.
/// Order is determined by registration order of scheme clients.
pub struct FirstMatch;

impl PaymentSelector for FirstMatch {
    fn select<'a, T: PaymentCandidateLike>(&self, candidates: &'a [T]) -> Option<&'a T> {
        candidates.first()
    }
}

/// Selector that prefers chains matching patterns in priority order.
/// The first pattern in the vector has the highest priority, the last has the lowest.
#[allow(dead_code)]
pub struct PreferChain(Vec<ChainIdPattern>);

impl PreferChain {
    pub fn new<P: Into<Vec<ChainIdPattern>>>(patterns: P) -> Self {
        Self(patterns.into())
    }

    pub fn chain<P: Into<Vec<ChainIdPattern>>>(mut self, patterns: P) -> PreferChain {
        PreferChain(self.0.into_iter().chain(patterns.into()).collect())
    }
}

impl PaymentSelector for PreferChain {
    fn select<'a, T: PaymentCandidateLike>(&self, candidates: &'a [T]) -> Option<&'a T> {
        // Try each pattern in priority order
        for pattern in &self.0 {
            if let Some(candidate) = candidates.iter().find(|c| pattern.matches(c.chain_id())) {
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
    fn select<'a, T: PaymentCandidateLike>(&self, candidates: &'a [T]) -> Option<&'a T> {
        candidates.iter().find(|c| c.amount() <= self.0)
    }
}
