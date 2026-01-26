//! Type definitions for the V1 Solana "exact" payment scheme.
//!
//! This module defines the wire format types for SPL Token based payments
//! on Solana using the V1 x402 protocol.

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use solana_pubkey::Pubkey;
use x402_types::proto::util::U64String;
use x402_types::{lit_str, proto};

use crate::chain::solana::Address;

lit_str!(ExactScheme, "exact");

/// Phantom Lighthouse program ID - security program injected by Phantom wallet on mainnet
/// See: https://github.com/coinbase/x402/issues/828
pub static PHANTOM_LIGHTHOUSE_PROGRAM: Lazy<Pubkey> = Lazy::new(|| {
    "L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95"
        .parse()
        .expect("Invalid Lighthouse program ID")
});

pub type VerifyRequest = proto::v1::VerifyRequest<PaymentPayload, PaymentRequirements>;
pub type SettleRequest = VerifyRequest;
pub type PaymentPayload = proto::v1::PaymentPayload<ExactScheme, ExactSolanaPayload>;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactSolanaPayload {
    pub transaction: String,
}

pub type PaymentRequirements =
    proto::v1::PaymentRequirements<ExactScheme, U64String, Address, SupportedPaymentKindExtra>;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SupportedPaymentKindExtra {
    pub fee_payer: Address,
}

/// Configuration for V1 Solana Exact Facilitator
///
/// Controls transaction verification behavior, including support for
/// additional instructions from third-party wallets like Phantom.
///
/// By default, the Phantom Lighthouse program is allowed to support
/// Phantom wallet users on mainnet.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct V1SolanaExactFacilitatorConfig {
    /// Allow additional instructions beyond the required ones
    /// Default: true (to support Phantom Lighthouse)
    #[serde(default = "default_allow_additional_instructions")]
    pub allow_additional_instructions: bool,

    /// Maximum number of instructions allowed in a transaction
    /// Default: 10
    #[serde(default = "default_max_instruction_count")]
    pub max_instruction_count: usize,

    /// Explicitly allowed program IDs for additional instructions.
    /// Only checked if allow_additional_instructions is true.
    /// Uses Solana Address type which deserializes from base58 strings.
    ///
    /// Default: [Phantom Lighthouse program]
    ///
    /// SECURITY: If this list is empty and allow_additional_instructions is true,
    /// ALL additional instructions will be rejected. You must explicitly whitelist
    /// the programs you want to allow.
    #[serde(default = "default_allowed_program_ids")]
    pub allowed_program_ids: Vec<Address>,

    /// Blocked program IDs (always rejected, takes precedence over allowed).
    /// Uses Solana Address type which deserializes from base58 strings.
    #[serde(default)]
    pub blocked_program_ids: Vec<Address>,

    /// SECURITY: Require fee payer is NOT present in any instruction's accounts
    /// Default: true - strongly recommended to keep this enabled
    #[serde(default = "default_require_fee_payer_not_in_instructions")]
    pub require_fee_payer_not_in_instructions: bool,
}

fn default_allow_additional_instructions() -> bool {
    true
}

fn default_max_instruction_count() -> usize {
    10
}

fn default_allowed_program_ids() -> Vec<Address> {
    vec![Address::new(*PHANTOM_LIGHTHOUSE_PROGRAM)]
}

fn default_require_fee_payer_not_in_instructions() -> bool {
    true
}

impl Default for V1SolanaExactFacilitatorConfig {
    fn default() -> Self {
        Self {
            allow_additional_instructions: default_allow_additional_instructions(),
            max_instruction_count: default_max_instruction_count(),
            allowed_program_ids: default_allowed_program_ids(),
            blocked_program_ids: Vec::new(),
            require_fee_payer_not_in_instructions: default_require_fee_payer_not_in_instructions(),
        }
    }
}

impl V1SolanaExactFacilitatorConfig {
    /// Check if a program ID is in the blocked list
    pub fn is_blocked(&self, program_id: &Pubkey) -> bool {
        self.blocked_program_ids
            .iter()
            .any(|addr| addr.pubkey() == program_id)
    }

    /// Check if a program ID is in the allowed list.
    ///
    /// SECURITY: If the allowed list is empty, NO programs are allowed.
    /// This follows the principle of least privilege - you must explicitly
    /// whitelist programs you want to accept.
    pub fn is_allowed(&self, program_id: &Pubkey) -> bool {
        self.allowed_program_ids
            .iter()
            .any(|addr| addr.pubkey() == program_id)
    }
}
