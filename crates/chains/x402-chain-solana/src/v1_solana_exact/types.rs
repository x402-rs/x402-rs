//! Type definitions for the V1 Solana "exact" payment scheme.
//!
//! This module defines the wire format types for SPL Token based payments
//! on Solana using the V1 x402 protocol.

use serde::{Deserialize, Serialize};
use solana_pubkey::{Pubkey, pubkey};
use x402_types::proto::PaymentVerificationError;
use x402_types::proto::util::U64String;
use x402_types::{lit_str, proto};

use crate::chain::Address;
#[cfg(feature = "facilitator")]
use crate::chain::{SolanaChainProviderError, SolanaChainProviderLike};

#[cfg(feature = "facilitator")]
use solana_commitment_config::CommitmentConfig;
#[cfg(any(feature = "client", feature = "facilitator"))]
use solana_message::compiled_instruction::CompiledInstruction;
#[cfg(any(feature = "client", feature = "facilitator"))]
use solana_signature::Signature;
#[cfg(any(feature = "client", feature = "facilitator"))]
use solana_signer::Signer;
#[cfg(any(feature = "client", feature = "facilitator"))]
use solana_transaction::versioned::VersionedTransaction;
#[cfg(any(feature = "client", feature = "facilitator"))]
use x402_types::util::Base64Bytes;

lit_str!(ExactScheme, "exact");

/// SPL Memo program ID - used to add transaction uniqueness and prevent duplicate transaction attacks
/// See: https://github.com/coinbase/x402/issues/828
pub static MEMO_PROGRAM_PUBKEY: Pubkey = pubkey!("MemoSq4gqABAXKb96qnH8TysNcWxMyWCqXgDLGmfcHr");

/// Phantom Lighthouse program ID - security program injected by Phantom wallet on mainnet
/// See: https://github.com/coinbase/x402/issues/828
pub static PHANTOM_LIGHTHOUSE_PROGRAM_PUBKEY: Pubkey =
    pubkey!("L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95");

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

pub const ATA_PROGRAM_PUBKEY: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

#[cfg(any(feature = "client", feature = "facilitator"))]
pub struct InstructionInt {
    index: usize,
    instruction: CompiledInstruction,
    account_keys: Vec<Pubkey>,
}

#[cfg(any(feature = "client", feature = "facilitator"))]
pub struct TransactionInt {
    inner: VersionedTransaction,
}

#[cfg(any(feature = "client", feature = "facilitator"))]
impl TransactionInt {
    pub fn new(transaction: VersionedTransaction) -> Self {
        Self { inner: transaction }
    }
    pub fn inner(&self) -> &VersionedTransaction {
        &self.inner
    }
    pub fn instruction(&self, index: usize) -> Result<InstructionInt, SolanaExactError> {
        let instruction = self
            .inner
            .message
            .instructions()
            .get(index)
            .cloned()
            .ok_or(SolanaExactError::NoInstructionAtIndex(index))?;
        let account_keys = self.inner.message.static_account_keys().to_vec();

        Ok(InstructionInt {
            index,
            instruction,
            account_keys,
        })
    }

    pub fn is_fully_signed(&self) -> bool {
        let num_required = self.inner.message.header().num_required_signatures;
        if self.inner.signatures.len() < num_required as usize {
            return false;
        }
        let default = Signature::default();
        for signature in self.inner.signatures.iter() {
            if default.eq(signature) {
                return false;
            }
        }
        true
    }

    #[cfg(feature = "facilitator")]
    pub fn sign<P: SolanaChainProviderLike>(
        self,
        provider: &P,
    ) -> Result<Self, SolanaChainProviderError> {
        let tx = provider.sign(self.inner)?;
        Ok(Self { inner: tx })
    }

    /// Sign the transaction with any Signer.
    /// This is used by the client to sign transactions before sending to the facilitator.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn sign_with_keypair<S: Signer>(self, signer: &S) -> Result<Self, TransactionSignError> {
        let mut tx = self.inner;
        let msg_bytes = tx.message.serialize();
        let signature = signer
            .try_sign_message(msg_bytes.as_slice())
            .map_err(|e| TransactionSignError(format!("{e}")))?;

        // Required signatures are the first N account keys
        let num_required = tx.message.header().num_required_signatures as usize;
        let static_keys = tx.message.static_account_keys();

        // Find signer's position
        let pos = static_keys[..num_required]
            .iter()
            .position(|k| *k == signer.pubkey())
            .ok_or(TransactionSignError(
                "Signer not found in required signers".to_string(),
            ))?;

        // Ensure signature vector is large enough, then place the signature
        if tx.signatures.len() < num_required {
            tx.signatures.resize(num_required, Signature::default());
        }
        tx.signatures[pos] = signature;
        Ok(Self { inner: tx })
    }

    #[cfg(feature = "facilitator")]
    pub async fn send_and_confirm<P: SolanaChainProviderLike>(
        &self,
        provider: &P,
        commitment_config: CommitmentConfig,
    ) -> Result<Signature, SolanaChainProviderError> {
        provider
            .send_and_confirm(&self.inner, commitment_config)
            .await
    }

    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn as_base64(&self) -> Result<String, TransactionToB64Error> {
        let bytes =
            bincode::serialize(&self.inner).map_err(|e| TransactionToB64Error(format!("{e}")))?;
        let base64_bytes = Base64Bytes::encode(bytes);
        let string = String::from_utf8(base64_bytes.0.into_owned())
            .map_err(|e| TransactionToB64Error(format!("{e}")))?;
        Ok(string)
    }
}

#[cfg(any(feature = "client", feature = "facilitator"))]
impl InstructionInt {
    pub fn has_data(&self) -> bool {
        !self.instruction.data.is_empty()
    }

    pub fn has_accounts(&self) -> bool {
        !self.instruction.accounts.is_empty()
    }

    pub fn data_slice(&self) -> &[u8] {
        self.instruction.data.as_slice()
    }

    pub fn assert_not_empty(&self) -> Result<(), SolanaExactError> {
        if !self.has_data() || !self.has_accounts() {
            return Err(SolanaExactError::EmptyInstructionAtIndex(self.index));
        }
        Ok(())
    }

    pub fn program_id(&self) -> Pubkey {
        *self.instruction.program_id(self.account_keys.as_slice())
    }

    pub fn account(&self, index: u8) -> Result<Pubkey, SolanaExactError> {
        let account_index = self
            .instruction
            .accounts
            .get(index as usize)
            .cloned()
            .ok_or(SolanaExactError::NoAccountAtIndex(index))?;
        let pubkey = self
            .account_keys
            .get(account_index as usize)
            .cloned()
            .ok_or(SolanaExactError::NoAccountAtIndex(index))?;
        Ok(pubkey)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Can not encode transaction to base64: {0}")]
pub struct TransactionToB64Error(String);

#[derive(Debug, thiserror::Error)]
pub enum SolanaExactError {
    #[error("Can not decode transaction: {0}")]
    TransactionDecoding(String),
    #[error("Compute unit limit exceeds facilitator maximum")]
    MaxComputeUnitLimitExceeded,
    #[error("Compute unit price exceeds facilitator maximum")]
    MaxComputeUnitPriceExceeded,
    #[error("Too few instructions in transaction")]
    TooFewInstructions,
    #[error("Additional instructions not allowed")]
    AdditionalInstructionsNotAllowed,
    #[error("Instruction count exceeds maximum: {0}")]
    InstructionCountExceedsMax(usize),
    #[error("Blocked program in transaction: {0}")]
    BlockedProgram(Pubkey),
    #[error("Program not in allowed list: {0}")]
    ProgramNotAllowed(Pubkey),
    #[error("CreateATA instruction not supported - destination ATA must exist")]
    CreateATANotSupported,
    #[error("Fee payer included in instruction accounts")]
    FeePayerIncludedInInstructionAccounts,
    #[error("Fee payer found transferring funds")]
    FeePayerTransferringFunds,
    #[error("Instruction at index {0} not found")]
    NoInstructionAtIndex(usize),
    #[error("No account at index {0}")]
    NoAccountAtIndex(u8),
    #[error("Empty instruction at index {0}")]
    EmptyInstructionAtIndex(usize),
    #[error("Invalid compute limit instruction")]
    InvalidComputeLimitInstruction,
    #[error("Invalid compute price instruction")]
    InvalidComputePriceInstruction,
    #[error("Invalid token instruction")]
    InvalidTokenInstruction,
    #[error("Missing sender account in transaction")]
    MissingSenderAccount,
}

impl From<SolanaExactError> for PaymentVerificationError {
    fn from(e: SolanaExactError) -> Self {
        match e {
            SolanaExactError::TransactionDecoding(_) => {
                PaymentVerificationError::InvalidFormat(e.to_string())
            }
            SolanaExactError::MaxComputeUnitLimitExceeded
            | SolanaExactError::MaxComputeUnitPriceExceeded
            | SolanaExactError::TooFewInstructions
            | SolanaExactError::AdditionalInstructionsNotAllowed
            | SolanaExactError::InstructionCountExceedsMax(_)
            | SolanaExactError::BlockedProgram(_)
            | SolanaExactError::ProgramNotAllowed(_)
            | SolanaExactError::CreateATANotSupported
            | SolanaExactError::FeePayerIncludedInInstructionAccounts
            | SolanaExactError::NoInstructionAtIndex(_)
            | SolanaExactError::InvalidComputeLimitInstruction
            | SolanaExactError::NoAccountAtIndex(_)
            | SolanaExactError::InvalidTokenInstruction
            | SolanaExactError::EmptyInstructionAtIndex(_)
            | SolanaExactError::FeePayerTransferringFunds
            | SolanaExactError::MissingSenderAccount
            | SolanaExactError::InvalidComputePriceInstruction => {
                PaymentVerificationError::TransactionSimulation(e.to_string())
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Can not sign transaction: {0}")]
pub struct TransactionSignError(pub String);
