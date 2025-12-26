use alloy_primitives::U256;
use crate::chain::ChainId;

pub struct PaymentCandidate {
    pub chain_id: ChainId,
    pub asset: String,
    pub amount: U256,
    pub scheme: String,
    pub x402_version: u8,
    pub pay_to: String,
}

// ============================================================================
// PaymentSelector - Selection strategy
// ============================================================================

/// Trait for selecting the best payment candidate from available options.
pub trait PaymentSelector: Send + Sync {
    fn select<'a>(&self, candidates: &'a [PaymentCandidate]) -> Option<&'a PaymentCandidate>;
}

/// Default selector: returns the first matching candidate.
/// Order is determined by registration order of scheme clients.
pub struct FirstMatch;

impl PaymentSelector for FirstMatch {
    fn select<'a>(&self, candidates: &'a [PaymentCandidate]) -> Option<&'a PaymentCandidate> {
        candidates.first()
    }
}

// /// Selector that prefers chains matching patterns in priority order.
// /// The first pattern in the vector has the highest priority, the last has the lowest.
// #[allow(dead_code)]
// pub struct PreferChain(Vec<ChainIdPattern>);
//
// impl PreferChain {
//     pub fn new<P: Into<Vec<ChainIdPattern>>>(patterns: P) -> Self {
//         Self(patterns.into())
//     }
//
//     pub fn chain<P: Into<Vec<ChainIdPattern>>>(mut self, patterns: P) -> PreferChain {
//         PreferChain(self.0.into_iter().chain(patterns.into()).collect())
//     }
// }
//
// impl PaymentSelector for PreferChain {
//     fn select<T: PaymentCandidateLike>(&self, candidates: &[T]) -> Option<&T> {
//         // Try each pattern in priority order
//         for pattern in &self.0 {
//             if let Some(candidate) = candidates.iter().find(|c| pattern.matches(c.chain_id())) {
//                 return Some(candidate);
//             }
//         }
//         // Fall back to first match if no patterns matched
//         candidates.first()
//     }
// }
//
// /// Selector that only accepts payments up to a maximum amount.
// #[allow(dead_code)]
// pub struct MaxAmount(pub U256);
//
// impl PaymentSelector for MaxAmount {
//     fn select<T: PaymentCandidateLike>(&self, candidates: &[T]) -> Option<&T> {
//         candidates.iter().find(|c| c.amount() <= self.0)
//     }
// }
