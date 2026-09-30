//! x402 v2 `exact` support for native BCH.

pub mod client;
pub mod facilitator;
pub mod server;
pub mod types;

pub use client::{
    BchHdDiscoveryOptions, BchSigner, BchWallet, BchWalletAddress, Secp256k1BchSigner,
    V2BchExactClient, V2BchExactWalletClient, discover_bch_hd_wallet_addresses,
};
pub use facilitator::{BchConfirmationStrategy, BchFacilitatorConfig, V2BchExactFacilitator};
pub use server::BchPaymentRequirements;
pub use types::{
    BchExtra, BchNftRequest, BchTokenRequest, BchTransactionNetwork, BchTransactionRequest,
    ExactBchPayload, ExactScheme, PaymentPayload, PaymentRequirements,
};

use x402_types::scheme::X402SchemeId;

/// Marker for the native BCH v2 exact scheme.
#[derive(Debug, Clone, Copy, Default)]
pub struct V2BchExact;

impl X402SchemeId for V2BchExact {
    fn namespace(&self) -> &str {
        crate::BCH_NAMESPACE
    }

    fn scheme(&self) -> &str {
        "exact"
    }
}
