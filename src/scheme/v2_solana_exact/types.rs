//! Type definitions for the V2 Solana "exact" payment scheme.
//!
//! This module re-exports types from V1 and defines V2-specific wire format
//! types for SPL Token based payments on Solana.

use x402_types::proto::util::U64String;
use x402_types::proto::v2;

use crate::chain::solana::Address;
use crate::scheme::v1_solana_exact::types::V1SolanaExactFacilitatorConfig;
use crate::scheme::v1_solana_exact::types::{ExactSolanaPayload, SupportedPaymentKindExtra};

pub use crate::scheme::v1_eip155_exact::types::ExactScheme;

/// Configuration for V2 Solana Exact facilitator - reuses V1 config
pub type V2SolanaExactFacilitatorConfig = V1SolanaExactFacilitatorConfig;

pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;
pub type SettleRequest = VerifyRequest;
pub type PaymentPayload = v2::PaymentPayload<PaymentRequirements, ExactSolanaPayload>;
pub type PaymentRequirements =
    v2::PaymentRequirements<ExactScheme, U64String, Address, SupportedPaymentKindExtra>;
