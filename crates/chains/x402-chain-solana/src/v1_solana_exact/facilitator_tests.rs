//! Tests for the V1 Solana Exact facilitator.
//!
//! These tests cover offline/pure validation functions that do not require
//! Solana RPC, following the same pattern as the Aptos and EIP-155 facilitator tests.

use solana_compute_budget_interface::ID as COMPUTE_BUDGET_PROGRAM;
use solana_message::compiled_instruction::CompiledInstruction;
use solana_message::VersionedMessage;
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;

use crate::chain::Address;
use crate::v1_solana_exact::facilitator::{
    V1SolanaExactFacilitatorConfig, validate_instructions, verify_compute_limit_instruction,
    verify_compute_price_instruction,
};
use crate::v1_solana_exact::types::{
    ATA_PROGRAM_PUBKEY, MEMO_PROGRAM_PUBKEY, PHANTOM_LIGHTHOUSE_PROGRAM_PUBKEY, SolanaExactError,
};

// ──────────────────────────────────────────────────
// Test helpers
// ──────────────────────────────────────────────────

/// Create a minimal VersionedTransaction with the given account keys and instructions.
///
/// Uses bincode deserialization of a hand-crafted legacy message to avoid
/// private field access in solana_message.
fn make_transaction(
    account_keys: Vec<Pubkey>,
    instructions: Vec<CompiledInstruction>,
) -> VersionedTransaction {
    use solana_message::legacy::Message as LegacyMessage;

    // Build legacy message using the public constructor
    let message = LegacyMessage::new_with_compiled_instructions(
        1,                  // num_required_signatures
        0,                  // num_readonly_signed_accounts
        account_keys.len() as u8 - 1, // num_readonly_unsigned_accounts
        account_keys,
        solana_message::Hash::default(),
        instructions,
    );
    VersionedTransaction {
        signatures: vec![Signature::default()],
        message: VersionedMessage::Legacy(message),
    }
}

/// Build a SetComputeUnitLimit instruction (discriminator 2, 4 bytes LE u32).
fn compute_limit_instruction(program_id_index: u8, limit: u32) -> CompiledInstruction {
    let mut data = vec![2u8]; // discriminator
    data.extend_from_slice(&limit.to_le_bytes());
    CompiledInstruction {
        program_id_index,
        accounts: vec![],
        data,
    }
}

/// Build a SetComputeUnitPrice instruction (discriminator 3, 8 bytes LE u64).
fn compute_price_instruction(program_id_index: u8, microlamports: u64) -> CompiledInstruction {
    let mut data = vec![3u8]; // discriminator
    data.extend_from_slice(&microlamports.to_le_bytes());
    CompiledInstruction {
        program_id_index,
        accounts: vec![],
        data,
    }
}

/// Build a dummy instruction for a given program index.
fn dummy_instruction(program_id_index: u8) -> CompiledInstruction {
    CompiledInstruction {
        program_id_index,
        accounts: vec![0],
        data: vec![1, 2, 3],
    }
}

/// Create a standard 3-instruction transaction: ComputeLimit + ComputePrice + Transfer
fn make_standard_transaction(
    transfer_program: Pubkey,
    compute_limit: u32,
    compute_price: u64,
) -> VersionedTransaction {
    let payer = Pubkey::new_unique();
    let account_keys = vec![payer, COMPUTE_BUDGET_PROGRAM, transfer_program];
    let instructions = vec![
        compute_limit_instruction(1, compute_limit),  // index 0: compute limit
        compute_price_instruction(1, compute_price),   // index 1: compute price
        dummy_instruction(2),                           // index 2: transfer
    ];
    make_transaction(account_keys, instructions)
}

// ──────────────────────────────────────────────────
// verify_compute_limit_instruction
// ──────────────────────────────────────────────────

#[test]
fn compute_limit_valid() {
    let tx = make_standard_transaction(Pubkey::new_unique(), 200_000, 1000);
    let result = verify_compute_limit_instruction(&tx, 0);
    assert_eq!(result.unwrap(), 200_000);
}

#[test]
fn compute_limit_max_u32() {
    let tx = make_standard_transaction(Pubkey::new_unique(), u32::MAX, 1000);
    let result = verify_compute_limit_instruction(&tx, 0);
    assert_eq!(result.unwrap(), u32::MAX);
}

#[test]
fn compute_limit_wrong_program() {
    let payer = Pubkey::new_unique();
    let wrong_program = Pubkey::new_unique();
    let account_keys = vec![payer, wrong_program];
    let instructions = vec![compute_limit_instruction(1, 200_000)];
    let tx = make_transaction(account_keys, instructions);
    let err = verify_compute_limit_instruction(&tx, 0).unwrap_err();
    assert!(matches!(err, SolanaExactError::InvalidComputeLimitInstruction));
}

#[test]
fn compute_limit_wrong_discriminator() {
    let payer = Pubkey::new_unique();
    let account_keys = vec![payer, COMPUTE_BUDGET_PROGRAM];
    // discriminator 3 instead of 2
    let mut data = vec![3u8];
    data.extend_from_slice(&200_000u32.to_le_bytes());
    let instructions = vec![CompiledInstruction {
        program_id_index: 1,
        accounts: vec![],
        data,
    }];
    let tx = make_transaction(account_keys, instructions);
    let err = verify_compute_limit_instruction(&tx, 0).unwrap_err();
    assert!(matches!(err, SolanaExactError::InvalidComputeLimitInstruction));
}

#[test]
fn compute_limit_wrong_data_length() {
    let payer = Pubkey::new_unique();
    let account_keys = vec![payer, COMPUTE_BUDGET_PROGRAM];
    // Only 3 bytes after discriminator instead of 4
    let instructions = vec![CompiledInstruction {
        program_id_index: 1,
        accounts: vec![],
        data: vec![2, 0, 0],
    }];
    let tx = make_transaction(account_keys, instructions);
    let err = verify_compute_limit_instruction(&tx, 0).unwrap_err();
    assert!(matches!(err, SolanaExactError::InvalidComputeLimitInstruction));
}

#[test]
fn compute_limit_index_out_of_bounds() {
    let tx = make_standard_transaction(Pubkey::new_unique(), 200_000, 1000);
    let err = verify_compute_limit_instruction(&tx, 99).unwrap_err();
    assert!(matches!(err, SolanaExactError::NoInstructionAtIndex(99)));
}

// ──────────────────────────────────────────────────
// verify_compute_price_instruction
// ──────────────────────────────────────────────────

#[test]
fn compute_price_valid() {
    let tx = make_standard_transaction(Pubkey::new_unique(), 200_000, 1000);
    assert!(verify_compute_price_instruction(5000, &tx, 1).is_ok());
}

#[test]
fn compute_price_at_max() {
    let tx = make_standard_transaction(Pubkey::new_unique(), 200_000, 5000);
    assert!(verify_compute_price_instruction(5000, &tx, 1).is_ok());
}

#[test]
fn compute_price_exceeds_max() {
    let tx = make_standard_transaction(Pubkey::new_unique(), 200_000, 10_000);
    let err = verify_compute_price_instruction(5000, &tx, 1).unwrap_err();
    assert!(matches!(err, SolanaExactError::MaxComputeUnitPriceExceeded));
}

#[test]
fn compute_price_wrong_program() {
    let payer = Pubkey::new_unique();
    let wrong_program = Pubkey::new_unique();
    let account_keys = vec![payer, wrong_program];
    let instructions = vec![compute_price_instruction(1, 1000)];
    let tx = make_transaction(account_keys, instructions);
    let err = verify_compute_price_instruction(5000, &tx, 0).unwrap_err();
    assert!(matches!(err, SolanaExactError::InvalidComputePriceInstruction));
}

#[test]
fn compute_price_index_out_of_bounds() {
    let tx = make_standard_transaction(Pubkey::new_unique(), 200_000, 1000);
    let err = verify_compute_price_instruction(5000, &tx, 99).unwrap_err();
    assert!(matches!(err, SolanaExactError::NoInstructionAtIndex(99)));
}

// ──────────────────────────────────────────────────
// validate_instructions
// ──────────────────────────────────────────────────

#[test]
fn validate_instructions_minimum_valid() {
    let tx = make_standard_transaction(Pubkey::new_unique(), 200_000, 1000);
    let config = V1SolanaExactFacilitatorConfig::default();
    assert!(validate_instructions(&tx, &config).is_ok());
}

#[test]
fn validate_instructions_too_few() {
    let payer = Pubkey::new_unique();
    let account_keys = vec![payer, COMPUTE_BUDGET_PROGRAM];
    let instructions = vec![
        compute_limit_instruction(1, 200_000),
        compute_price_instruction(1, 1000),
    ];
    let tx = make_transaction(account_keys, instructions);
    let config = V1SolanaExactFacilitatorConfig::default();
    let err = validate_instructions(&tx, &config).unwrap_err();
    assert!(matches!(err, SolanaExactError::TooFewInstructions));
}

#[test]
fn validate_instructions_exceeds_max_count() {
    let payer = Pubkey::new_unique();
    let transfer = Pubkey::new_unique();
    let allowed = MEMO_PROGRAM_PUBKEY;
    let account_keys = vec![payer, COMPUTE_BUDGET_PROGRAM, transfer, allowed];
    // 3 required + 8 additional = 11 (exceeds default max of 10)
    let mut instructions = vec![
        compute_limit_instruction(1, 200_000),
        compute_price_instruction(1, 1000),
        dummy_instruction(2),
    ];
    for _ in 0..8 {
        instructions.push(dummy_instruction(3)); // all memo program
    }
    let tx = make_transaction(account_keys, instructions);
    let config = V1SolanaExactFacilitatorConfig::default();
    let err = validate_instructions(&tx, &config).unwrap_err();
    assert!(matches!(err, SolanaExactError::InstructionCountExceedsMax(10)));
}

#[test]
fn validate_instructions_create_ata_at_index_2() {
    let payer = Pubkey::new_unique();
    let account_keys = vec![payer, COMPUTE_BUDGET_PROGRAM, ATA_PROGRAM_PUBKEY];
    let instructions = vec![
        compute_limit_instruction(1, 200_000),
        compute_price_instruction(1, 1000),
        dummy_instruction(2), // index 2 uses ATA program
    ];
    let tx = make_transaction(account_keys, instructions);
    let config = V1SolanaExactFacilitatorConfig::default();
    let err = validate_instructions(&tx, &config).unwrap_err();
    assert!(matches!(err, SolanaExactError::CreateATANotSupported));
}

#[test]
fn validate_instructions_additional_not_allowed() {
    let payer = Pubkey::new_unique();
    let transfer = Pubkey::new_unique();
    let extra = Pubkey::new_unique();
    let account_keys = vec![payer, COMPUTE_BUDGET_PROGRAM, transfer, extra];
    let instructions = vec![
        compute_limit_instruction(1, 200_000),
        compute_price_instruction(1, 1000),
        dummy_instruction(2),
        dummy_instruction(3), // additional instruction
    ];
    let tx = make_transaction(account_keys, instructions);
    let mut config = V1SolanaExactFacilitatorConfig::default();
    config.allow_additional_instructions = false;
    let err = validate_instructions(&tx, &config).unwrap_err();
    assert!(matches!(err, SolanaExactError::AdditionalInstructionsNotAllowed));
}

#[test]
fn validate_instructions_blocked_program() {
    let payer = Pubkey::new_unique();
    let transfer = Pubkey::new_unique();
    let blocked = Pubkey::new_unique();
    let account_keys = vec![payer, COMPUTE_BUDGET_PROGRAM, transfer, blocked];
    let instructions = vec![
        compute_limit_instruction(1, 200_000),
        compute_price_instruction(1, 1000),
        dummy_instruction(2),
        dummy_instruction(3), // blocked program
    ];
    let tx = make_transaction(account_keys, instructions);
    let mut config = V1SolanaExactFacilitatorConfig::default();
    config.blocked_program_ids = vec![Address::new(blocked)];
    let err = validate_instructions(&tx, &config).unwrap_err();
    assert!(matches!(err, SolanaExactError::BlockedProgram(_)));
}

#[test]
fn validate_instructions_program_not_allowed() {
    let payer = Pubkey::new_unique();
    let transfer = Pubkey::new_unique();
    let unknown = Pubkey::new_unique();
    let account_keys = vec![payer, COMPUTE_BUDGET_PROGRAM, transfer, unknown];
    let instructions = vec![
        compute_limit_instruction(1, 200_000),
        compute_price_instruction(1, 1000),
        dummy_instruction(2),
        dummy_instruction(3), // unknown program not in allowed list
    ];
    let tx = make_transaction(account_keys, instructions);
    let config = V1SolanaExactFacilitatorConfig::default();
    let err = validate_instructions(&tx, &config).unwrap_err();
    assert!(matches!(err, SolanaExactError::ProgramNotAllowed(_)));
}

#[test]
fn validate_instructions_allowed_program_passes() {
    let payer = Pubkey::new_unique();
    let transfer = Pubkey::new_unique();
    let account_keys = vec![payer, COMPUTE_BUDGET_PROGRAM, transfer, MEMO_PROGRAM_PUBKEY];
    let instructions = vec![
        compute_limit_instruction(1, 200_000),
        compute_price_instruction(1, 1000),
        dummy_instruction(2),
        dummy_instruction(3), // Memo program is in default allowed list
    ];
    let tx = make_transaction(account_keys, instructions);
    let config = V1SolanaExactFacilitatorConfig::default();
    assert!(validate_instructions(&tx, &config).is_ok());
}

// ──────────────────────────────────────────────────
// V1SolanaExactFacilitatorConfig
// ──────────────────────────────────────────────────

#[test]
fn config_default_values() {
    let config = V1SolanaExactFacilitatorConfig::default();
    assert!(config.allow_additional_instructions);
    assert_eq!(config.max_instruction_count, 10);
    assert!(config.require_fee_payer_not_in_instructions);
    assert!(config.blocked_program_ids.is_empty());
    assert_eq!(config.allowed_program_ids.len(), 2);
}

#[test]
fn config_default_allowed_includes_memo() {
    let config = V1SolanaExactFacilitatorConfig::default();
    assert!(config.is_allowed(&MEMO_PROGRAM_PUBKEY));
}

#[test]
fn config_default_allowed_includes_phantom_lighthouse() {
    let config = V1SolanaExactFacilitatorConfig::default();
    assert!(config.is_allowed(&PHANTOM_LIGHTHOUSE_PROGRAM_PUBKEY));
}

#[test]
fn config_is_blocked_returns_false_for_unknown() {
    let config = V1SolanaExactFacilitatorConfig::default();
    let unknown = Pubkey::new_unique();
    assert!(!config.is_blocked(&unknown));
}

#[test]
fn config_is_blocked_returns_true_when_listed() {
    let blocked = Pubkey::new_unique();
    let mut config = V1SolanaExactFacilitatorConfig::default();
    config.blocked_program_ids = vec![Address::new(blocked)];
    assert!(config.is_blocked(&blocked));
}

#[test]
fn config_is_allowed_returns_false_for_unknown() {
    let config = V1SolanaExactFacilitatorConfig::default();
    let unknown = Pubkey::new_unique();
    assert!(!config.is_allowed(&unknown));
}

#[test]
fn config_serde_roundtrip() {
    let config = V1SolanaExactFacilitatorConfig::default();
    let json = serde_json::to_value(&config).expect("should serialize");
    let deserialized: V1SolanaExactFacilitatorConfig =
        serde_json::from_value(json).expect("should deserialize");
    assert_eq!(deserialized.max_instruction_count, config.max_instruction_count);
    assert_eq!(deserialized.allow_additional_instructions, config.allow_additional_instructions);
}

// ──────────────────────────────────────────────────
// SolanaChainReference and Address serde
// ──────────────────────────────────────────────────

#[test]
fn chain_reference_from_chain_id() {
    use crate::chain::SolanaChainReference;
    use std::str::FromStr;
    use x402_types::chain::ChainId;

    // Solana mainnet genesis hash first 32 chars
    let chain_id = ChainId::from_str("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp").unwrap();
    let reference = SolanaChainReference::try_from(chain_id);
    assert!(reference.is_ok());
}

#[test]
fn chain_reference_wrong_namespace() {
    use crate::chain::SolanaChainReference;
    use std::str::FromStr;
    use x402_types::chain::ChainId;

    let chain_id = ChainId::from_str("eip155:8453").unwrap();
    let reference = SolanaChainReference::try_from(chain_id);
    assert!(reference.is_err());
}

#[test]
fn address_from_str_valid() {
    use crate::chain::Address;
    use std::str::FromStr;

    let addr = Address::from_str("9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM");
    assert!(addr.is_ok());
}

#[test]
fn address_from_str_invalid() {
    use crate::chain::Address;
    use std::str::FromStr;

    let addr = Address::from_str("not-a-valid-base58");
    assert!(addr.is_err());
}

#[test]
fn address_display_roundtrip() {
    use crate::chain::Address;
    use std::str::FromStr;

    let original = "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM";
    let addr = Address::from_str(original).unwrap();
    assert_eq!(addr.to_string(), original);
}
