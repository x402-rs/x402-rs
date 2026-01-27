//! Type definitions for the V2 EIP-155 "exact" payment scheme.
//!
//! This module re-exports types from V1 and defines V2-specific wire format
//! types for ERC-3009 based payments on EVM chains.

use crate::chain::{ChecksummedAddress, TokenAmount};
use x402_types::proto::v2;

pub use crate::v1_eip155_exact::types::ExactScheme;

use crate::v1_eip155_exact::types::{ExactEvmPayload, PaymentRequirementsExtra};

pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;
pub type SettleRequest = VerifyRequest;
pub type PaymentPayload = v2::PaymentPayload<PaymentRequirements, ExactEvmPayload>;
pub type PaymentRequirements =
    v2::PaymentRequirements<ExactScheme, TokenAmount, ChecksummedAddress, PaymentRequirementsExtra>;
