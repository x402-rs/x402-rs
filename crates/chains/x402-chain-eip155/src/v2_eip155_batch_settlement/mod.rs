//! V2 EIP-155 `batch-settlement` payment scheme implementation.
//!
//! `batch-settlement` is a capital-backed channel scheme for high-throughput,
//! low-cost EVM payments. Clients deposit funds into onchain escrow once and
//! sign off-chain **cumulative vouchers** per request; servers verify those
//! vouchers with fast signature checks and claim them onchain periodically in
//! batches. A single `claim` transaction can cover many channels at once and
//! only updates onchain accounting — claimed funds are later transferred to
//! the receiver via a separate `settle` operation that sweeps many claims into
//! one token transfer.
//!
//! See `docs/specs/schemes/batch-settlement/scheme_batch_settlement_evm.md`
//! (mirrored from the upstream `x402-foundation/x402` repo) for the full spec.
//!
//! # Module Layout
//!
//! - [`constants`]   — canonical contract addresses + EIP-712 domain metadata
//! - [`errors`]      — wire-format error code constants
//! - [`encoding`]    — calldata-encoding helpers for deposit collectors
//! - [`types`]       — wire types (payloads, channel config, vouchers, …)
//! - [`facilitator`] — verify / settle / supported dispatcher (gated behind
//!   the `facilitator` feature)
//!
//! # Scheme Identifier
//!
//! The blueprint registers itself as `v2-eip155-batch-settlement`.

pub mod constants;
pub mod encoding;
pub mod errors;
pub mod types;

pub use types::{
    AssetTransferMethod, BatchSettlementPayload, BatchSettlementPaymentRequirementsExtra,
    BatchSettlementRefundPayload, BatchSettlementScheme, ChannelConfig, ChannelStateExtra,
    ClaimPayload, DepositAuthorization, DepositPayload, DepositSegment, EnrichedRefundPayload,
    Erc3009Authorization, PaymentPayload, PaymentRequirements, Permit2Authorization,
    Permit2Permitted, Permit2Witness, RefundPayload, SettlePayload, SettleRequest, U128String,
    U256String, VerifyRequest, VoucherClaim, VoucherClaimVoucher, VoucherFields, VoucherPayload,
    VoucherStateExtra,
};

#[cfg(feature = "facilitator")]
pub mod facilitator;
#[cfg(feature = "facilitator")]
pub use facilitator::{V2Eip155BatchSettlementConfig, V2Eip155BatchSettlementFacilitator};

use x402_types::scheme::X402SchemeId;

/// Scheme identifier blueprint for `v2-eip155-batch-settlement`.
///
/// This unit struct is the public entry point for registering the scheme with
/// a [`x402_types::scheme::SchemeBlueprints`] registry. The facilitator-side
/// implementation lives in [`facilitator`] (gated behind the `facilitator`
/// feature).
pub struct V2Eip155BatchSettlement;

impl X402SchemeId for V2Eip155BatchSettlement {
    fn namespace(&self) -> &str {
        "eip155"
    }

    fn scheme(&self) -> &str {
        BatchSettlementScheme.as_ref()
    }
}
