pub mod client;
pub mod types;

use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_client::rpc_response::UiTransactionError;
use solana_commitment_config::CommitmentConfig;
use solana_compute_budget_interface::ID as ComputeBudgetInstructionId;
use solana_message::compiled_instruction::CompiledInstruction;
use solana_pubkey::{Pubkey, pubkey};
use solana_signature::Signature;
use solana_signer::Signer;
use solana_transaction::TransactionError;
use solana_transaction::versioned::VersionedTransaction;
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tracing_core::Level;

use crate::chain::solana::{Address, SolanaChainProvider, SolanaChainProviderError};
use crate::chain::{ChainId, ChainProvider, ChainProviderOps};
use crate::proto;
use crate::proto::PaymentVerificationError;
use crate::scheme::v1_solana_exact::types::SupportedPaymentKindExtra;
use crate::scheme::{
    X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError, X402SchemeId,
};
use crate::util::Base64Bytes;

pub const ATA_PROGRAM_PUBKEY: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

pub struct V1SolanaExact;

impl X402SchemeId for V1SolanaExact {
    fn x402_version(&self) -> u8 {
        1
    }

    fn namespace(&self) -> &str {
        "solana"
    }

    fn scheme(&self) -> &str {
        types::ExactScheme.as_ref()
    }
}

impl X402SchemeFacilitatorBuilder for V1SolanaExact {
    fn build(
        &self,
        provider: ChainProvider,
        _config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn Error>> {
        let provider = if let ChainProvider::Solana(provider) = provider {
            provider
        } else {
            return Err("V1SolanaExact::build: provider must be a SolanaChainProvider".into());
        };
        Ok(Box::new(V1SolanaExactFacilitator { provider }))
    }
}

pub struct V1SolanaExactFacilitator {
    provider: Arc<SolanaChainProvider>,
}

#[async_trait::async_trait]
impl X402SchemeFacilitator for V1SolanaExactFacilitator {
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError> {
        let request = types::VerifyRequest::from_proto(request.clone())?;
        let verification = verify_transfer(&self.provider, &request).await?;
        Ok(proto::v1::VerifyResponse::valid(verification.payer.to_string()).into())
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, X402SchemeFacilitatorError> {
        let request = types::SettleRequest::from_proto(request.clone())?;
        let verification = verify_transfer(&self.provider, &request).await?;
        let payer = verification.payer.to_string();
        let tx_sig = settle_transaction(&self.provider, verification).await?;
        Ok(proto::v1::SettleResponse::Success {
            payer,
            transaction: tx_sig.to_string(),
            network: self.provider.chain_id().to_string(),
        }
        .into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError> {
        let chain_id = self.provider.chain_id();
        let kinds: Vec<proto::SupportedPaymentKind> = {
            let mut kinds = Vec::with_capacity(1);
            let fee_payer = self.provider.fee_payer();
            let extra =
                Some(serde_json::to_value(SupportedPaymentKindExtra { fee_payer }).unwrap());
            let network = chain_id.as_network_name();
            if let Some(network) = network {
                kinds.push(proto::SupportedPaymentKind {
                    x402_version: proto::v1::X402Version1.into(),
                    scheme: types::ExactScheme.to_string(),
                    network: network.to_string(),
                    extra,
                });
            }
            kinds
        };
        let signers = {
            let mut signers = HashMap::with_capacity(1);
            signers.insert(chain_id, self.provider.signer_addresses());
            signers
        };
        Ok(proto::SupportedResponse {
            kinds,
            extensions: Vec::new(),
            signers,
        })
    }
}

pub struct InstructionInt {
    index: usize,
    instruction: CompiledInstruction,
    account_keys: Vec<Pubkey>,
}

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

pub struct TransactionInt {
    inner: VersionedTransaction,
}

impl TransactionInt {
    pub fn new(transaction: VersionedTransaction) -> Self {
        Self { inner: transaction }
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

    pub fn sign(self, provider: &SolanaChainProvider) -> Result<Self, SolanaChainProviderError> {
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

    pub async fn send_and_confirm(
        &self,
        provider: &SolanaChainProvider,
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

#[derive(Debug, thiserror::Error)]
#[error("Can not encode transaction to base64: {0}")]
pub struct TransactionToB64Error(String);

#[derive(Debug, thiserror::Error)]
#[error("Can not sign transaction: {0}")]
pub struct TransactionSignError(pub String);

pub struct VerifyTransferResult {
    pub payer: Address,
    pub transaction: VersionedTransaction,
}

#[derive(Debug)]
pub struct TransferCheckedInstruction {
    pub amount: u64,
    pub source: Pubkey,
    pub destination: Pubkey,
    pub authority: Pubkey,
    pub token_program: Pubkey,
}

pub fn verify_compute_limit_instruction(
    transaction: &VersionedTransaction,
    instruction_index: usize,
) -> Result<u32, SolanaExactError> {
    let instructions = transaction.message.instructions();
    let instruction = instructions
        .get(instruction_index)
        .ok_or(SolanaExactError::NoInstructionAtIndex(instruction_index))?;
    let account = instruction.program_id(transaction.message.static_account_keys());
    let data = instruction.data.as_slice();

    // Verify program ID, discriminator, and data length (1 byte discriminator + 4 bytes u32)
    if ComputeBudgetInstructionId.ne(account)
        || data.first().cloned().unwrap_or(0) != 2
        || data.len() != 5
    {
        return Err(SolanaExactError::InvalidComputeLimitInstruction);
    }

    // Parse compute unit limit (u32 in little-endian)
    let mut buf = [0u8; 4];
    buf.copy_from_slice(&data[1..5]);
    let compute_units = u32::from_le_bytes(buf);

    Ok(compute_units)
}

pub fn verify_compute_price_instruction(
    max_compute_unit_price: u64,
    transaction: &VersionedTransaction,
    instruction_index: usize,
) -> Result<(), SolanaExactError> {
    let instructions = transaction.message.instructions();
    let instruction = instructions
        .get(instruction_index)
        .ok_or(SolanaExactError::NoInstructionAtIndex(instruction_index))?;
    let account = instruction.program_id(transaction.message.static_account_keys());
    let compute_budget = solana_compute_budget_interface::ID;
    let data = instruction.data.as_slice();
    if compute_budget.ne(account) || data.first().cloned().unwrap_or(0) != 3 || data.len() != 9 {
        return Err(SolanaExactError::InvalidComputePriceInstruction);
    }
    // It is ComputeBudgetInstruction definitely by now!
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&data[1..]);
    let microlamports = u64::from_le_bytes(buf);
    if microlamports > max_compute_unit_price {
        return Err(SolanaExactError::MaxComputeUnitPriceExceeded);
    }
    Ok(())
}

pub fn verify_create_ata_instruction(
    transaction: &VersionedTransaction,
    index: usize,
    transfer_requirement: &TransferRequirement,
) -> Result<(), PaymentVerificationError> {
    let tx = TransactionInt::new(transaction.clone());
    let instruction = tx.instruction(index)?;
    instruction.assert_not_empty()?;

    // Verify program ID is the Associated Token Account Program
    let program_id = instruction.program_id();
    if program_id != ATA_PROGRAM_PUBKEY {
        return Err(SolanaExactError::InvalidCreateATAInstruction.into());
    }

    // Verify instruction discriminator
    // The ATA program's Create instruction has discriminator 0 (Create) or 1 (CreateIdempotent)
    let data = instruction.data_slice();
    if data.is_empty() || (data[0] != 0 && data[0] != 1) {
        return Err(SolanaExactError::InvalidCreateATAInstruction.into());
    }

    // Verify account count (must have at least 6 accounts)
    if instruction.instruction.accounts.len() < 6 {
        return Err(SolanaExactError::InvalidCreateATAInstruction.into());
    }

    // Payer = 0
    instruction.account(0)?;
    // ATA = 1
    instruction.account(1)?;
    // Owner = 2
    let owner = instruction.account(2)?;
    // Mint = 3
    let mint = instruction.account(3)?;
    // SystemProgram = 4
    instruction.account(4)?;
    // TokenProgram = 5
    instruction.account(5)?;

    // verify that the ATA is created for the expected payee
    if Address::new(owner) != *transfer_requirement.pay_to {
        return Err(PaymentVerificationError::RecipientMismatch);
    }
    if Address::new(mint) != *transfer_requirement.asset {
        return Err(PaymentVerificationError::AssetMismatch);
    }
    Ok(())
}

pub async fn verify_transfer(
    provider: &SolanaChainProvider,
    request: &types::VerifyRequest,
) -> Result<VerifyTransferResult, PaymentVerificationError> {
    let payload = &request.payment_payload;
    let requirements = &request.payment_requirements;

    // Assert valid payment START
    let chain_id = provider.chain_id();
    let payload_chain_id = ChainId::from_network_name(&payload.network)
        .ok_or(PaymentVerificationError::UnsupportedChain)?;
    if payload_chain_id != chain_id {
        return Err(PaymentVerificationError::ChainIdMismatch);
    }
    let requirements_chain_id = ChainId::from_network_name(&requirements.network)
        .ok_or(PaymentVerificationError::UnsupportedChain)?;
    if requirements_chain_id != chain_id {
        return Err(PaymentVerificationError::ChainIdMismatch);
    }
    let transaction_b64_string = payload.payload.transaction.clone();
    let transfer_requirement = TransferRequirement {
        pay_to: &requirements.pay_to,
        asset: &requirements.asset,
        amount: requirements.max_amount_required.inner(),
    };
    let result =
        verify_transaction(provider, transaction_b64_string, &transfer_requirement).await?;
    Ok(result)
}

pub async fn verify_transaction(
    provider: &SolanaChainProvider,
    transaction_b64_string: String,
    transfer_requirement: &TransferRequirement<'_>,
) -> Result<VerifyTransferResult, PaymentVerificationError> {
    let bytes = Base64Bytes::from(transaction_b64_string.as_bytes())
        .decode()
        .map_err(|e| SolanaExactError::TransactionDecoding(e.to_string()))?;
    let transaction = bincode::deserialize::<VersionedTransaction>(bytes.as_slice())
        .map_err(|e| SolanaExactError::TransactionDecoding(e.to_string()))?;

    // perform transaction introspection to validate the transaction structure and details
    let instructions = transaction.message.instructions();
    let compute_units = verify_compute_limit_instruction(&transaction, 0)?;
    if compute_units > provider.max_compute_unit_limit() {
        return Err(SolanaExactError::MaxComputeUnitLimitExceeded.into());
    }
    tracing::debug!(compute_units = compute_units, "Verified compute unit limit");
    verify_compute_price_instruction(provider.max_compute_unit_price(), &transaction, 1)?;
    let transfer_instruction = if instructions.len() == 3 {
        // verify that the transfer instruction is valid
        // this expects the destination ATA to already exist
        verify_transfer_instruction(provider, &transaction, 2, transfer_requirement, false).await?
    } else if instructions.len() == 4 {
        // verify that the transfer instruction is valid
        // this expects the destination ATA to be created in the same transaction
        verify_create_ata_instruction(&transaction, 2, transfer_requirement)?;
        verify_transfer_instruction(provider, &transaction, 3, transfer_requirement, true).await?
    } else {
        return Err(SolanaExactError::InvalidTransactionInstructionsCount.into());
    };

    // Rule 2: Fee payer safety check
    // Verify that the fee payer is not included in any instruction's accounts
    // This single check covers all cases: authority, source, or any other role
    let fee_payer_pubkey = provider.pubkey();
    for instruction in transaction.message.instructions().iter() {
        for account_idx in instruction.accounts.iter() {
            let account = transaction
                .message
                .static_account_keys()
                .get(*account_idx as usize)
                .ok_or(SolanaExactError::NoAccountAtIndex(*account_idx))?;

            if *account == fee_payer_pubkey {
                return Err(SolanaExactError::FeePayerIncludedInInstructionAccounts.into());
            }
        }
    }

    let tx = TransactionInt::new(transaction.clone()).sign(provider)?;
    let cfg = RpcSimulateTransactionConfig {
        sig_verify: false,
        replace_recent_blockhash: false,
        commitment: Some(CommitmentConfig::confirmed()),
        encoding: None, // optional; client handles encoding
        accounts: None,
        inner_instructions: false,
        min_context_slot: None,
    };
    provider
        .simulate_transaction_with_config(&tx.inner, cfg)
        .await?;
    let payer: Address = transfer_instruction.authority.into();
    Ok(VerifyTransferResult { payer, transaction })
}

pub struct TransferRequirement<'a> {
    pub asset: &'a Address,
    pub pay_to: &'a Address,
    pub amount: u64,
}

pub async fn verify_transfer_instruction(
    provider: &SolanaChainProvider,
    transaction: &VersionedTransaction,
    instruction_index: usize,
    transfer_requirement: &TransferRequirement<'_>,
    has_dest_ata: bool,
) -> Result<TransferCheckedInstruction, PaymentVerificationError> {
    let tx = TransactionInt::new(transaction.clone());
    let instruction = tx.instruction(instruction_index)?;
    instruction.assert_not_empty()?;
    let program_id = instruction.program_id();
    let transfer_checked_instruction = if spl_token::ID.eq(&program_id) {
        let token_instruction =
            spl_token::instruction::TokenInstruction::unpack(instruction.data_slice())
                .map_err(|_| SolanaExactError::InvalidTokenInstruction)?;
        let amount = match token_instruction {
            spl_token::instruction::TokenInstruction::TransferChecked {
                amount,
                decimals: _,
            } => amount,
            _ => return Err(SolanaExactError::InvalidTokenInstruction.into()),
        };
        // Source = 0
        let source = instruction.account(0)?;
        // Mint = 1
        let _mint = instruction.account(1)?;
        // Destination = 2
        let destination = instruction.account(2)?;
        // Authority = 3
        let authority = instruction.account(3)?;
        TransferCheckedInstruction {
            amount,
            source,
            destination,
            authority,
            token_program: spl_token::ID,
        }
    } else if spl_token_2022::ID.eq(&program_id) {
        let token_instruction =
            spl_token_2022::instruction::TokenInstruction::unpack(instruction.data_slice())
                .map_err(|_| SolanaExactError::InvalidTokenInstruction)?;
        let amount = match token_instruction {
            spl_token_2022::instruction::TokenInstruction::TransferChecked {
                amount,
                decimals: _,
            } => amount,
            _ => return Err(SolanaExactError::InvalidTokenInstruction.into()),
        };
        // Source = 0
        let source = instruction.account(0)?;
        // Mint = 1
        let _mint = instruction.account(1)?;
        // Destination = 2
        let destination = instruction.account(2)?;
        // Authority = 3
        let authority = instruction.account(3)?;
        TransferCheckedInstruction {
            amount,
            source,
            destination,
            authority,
            token_program: spl_token_2022::ID,
        }
    } else {
        return Err(SolanaExactError::InvalidTokenInstruction.into());
    };

    // Verify that the fee payer is not transferring funds (not the authority)
    let fee_payer_pubkey = provider.pubkey();
    if transfer_checked_instruction.authority == fee_payer_pubkey {
        return Err(SolanaExactError::FeePayerTransferringFunds.into());
    }

    let token_program = transfer_checked_instruction.token_program;
    // findAssociatedTokenPda
    let (ata, _) = Pubkey::find_program_address(
        &[
            transfer_requirement.pay_to.as_ref(),
            token_program.as_ref(),
            transfer_requirement.asset.as_ref(),
        ],
        &ATA_PROGRAM_PUBKEY,
    );
    if transfer_checked_instruction.destination != ata {
        return Err(PaymentVerificationError::RecipientMismatch);
    }
    let accounts = provider
        .get_multiple_accounts(&[transfer_checked_instruction.source, ata])
        .await?;
    let is_sender_missing = accounts.first().cloned().is_none_or(|a| a.is_none());
    if is_sender_missing {
        return Err(SolanaExactError::MissingSenderAccount.into());
    }
    let is_receiver_missing = accounts.get(1).cloned().is_none_or(|a| a.is_none());
    if is_receiver_missing && !has_dest_ata {
        return Err(PaymentVerificationError::RecipientMismatch);
    }
    let instruction_amount = transfer_checked_instruction.amount;
    if instruction_amount != transfer_requirement.amount {
        return Err(PaymentVerificationError::InvalidPaymentAmount);
    }
    Ok(transfer_checked_instruction)
}

pub async fn settle_transaction(
    provider: &SolanaChainProvider,
    verification: VerifyTransferResult,
) -> Result<Signature, SolanaChainProviderError> {
    let tx = TransactionInt::new(verification.transaction).sign(provider)?;
    // Verify if fully signed
    if !tx.is_fully_signed() {
        tracing::event!(Level::WARN, status = "failed", "undersigned transaction");
        return Err(SolanaChainProviderError::InvalidTransaction(
            UiTransactionError::from(TransactionError::SignatureFailure),
        ));
    }
    let tx_sig = tx
        .send_and_confirm(provider, CommitmentConfig::confirmed())
        .await?;
    Ok(tx_sig)
}

#[derive(Debug, thiserror::Error)]
pub enum SolanaExactError {
    #[error("Can not decode transaction: {0}")]
    TransactionDecoding(String),
    #[error("Compute unit limit exceeds facilitator maximum")]
    MaxComputeUnitLimitExceeded,
    #[error("Compute unit price exceeds facilitator maximum")]
    MaxComputeUnitPriceExceeded,
    #[error("Invalid transaction instructions count")]
    InvalidTransactionInstructionsCount,
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
    #[error("Invalid Create ATA instruction")]
    InvalidCreateATAInstruction,
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
            SolanaExactError::InvalidCreateATAInstruction
            | SolanaExactError::MaxComputeUnitLimitExceeded
            | SolanaExactError::MaxComputeUnitPriceExceeded
            | SolanaExactError::InvalidTransactionInstructionsCount
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

impl From<SolanaChainProviderError> for PaymentVerificationError {
    fn from(value: SolanaChainProviderError) -> Self {
        Self::TransactionSimulation(value.to_string())
    }
}
