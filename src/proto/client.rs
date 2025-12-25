use alloy_primitives::U256;

use crate::chain::ChainId;

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
