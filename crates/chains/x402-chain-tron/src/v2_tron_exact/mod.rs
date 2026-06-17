//! V2 TRON "exact" payment scheme implementation.
//!
//! Implements the x402 `scheme_exact` for TRON chains using TIP-712 (EIP-712 compatible)
//! typed data signing with either EIP-3009-style transferWithAuthorization or Permit2.

pub mod types;
pub use types::*;

#[cfg(feature = "facilitator")]
pub mod facilitator;
#[cfg(feature = "facilitator")]
pub use facilitator::*;

use x402_types::scheme::X402SchemeId;

/// The V2 TRON exact scheme marker.
pub struct V2TronExact;

impl X402SchemeId for V2TronExact {
    fn namespace(&self) -> &str {
        crate::TRON_NAMESPACE
    }

    fn scheme(&self) -> &str {
        ExactScheme.as_ref()
    }
}
