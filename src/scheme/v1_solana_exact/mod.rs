//! V1 Solana "exact" payment scheme implementation.
//!
//! This module implements the "exact" payment scheme for Solana using
//! the V1 x402 protocol. It uses SPL Token `TransferChecked` instructions
//! for token transfers.
//!
//! # Features
//!
//! - SPL Token and Token-2022 program support
//! - Compute budget instruction validation
//! - Transaction simulation before settlement
//! - Fee payer safety checks
//! - Configurable instruction allowlists/blocklists
//!
//! # Transaction Structure
//!
//! The expected transaction structure is:
//! - Index 0: `SetComputeUnitLimit` instruction
//! - Index 1: `SetComputeUnitPrice` instruction
//! - Index 2: `TransferChecked` instruction (SPL Token or Token-2022)
//! - Index 3+: Additional instructions (if allowed by configuration)
//!
//! # Usage
//!
//! ```ignore
//! use x402::scheme::v1_solana_exact::V1SolanaExact;
//! use x402::networks::{KnownNetworkSolana, USDC};
//!
//! // Create a price tag for 1 USDC on Solana mainnet
//! let usdc = USDC::solana_mainnet();
//! let price = V1SolanaExact::price_tag(
//!     "recipient_pubkey...",  // pay_to address
//!     usdc.amount(1_000_000),  // 1 USDC
//! );
//! ```

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
use std::sync::Arc;
use tracing_core::Level;
use x402_types::chain::ChainId;
use x402_types::chain::{ChainProviderOps, DeployedTokenAmount};
use x402_types::proto;
use x402_types::proto::PaymentVerificationError;
use x402_types::proto::v1;
use x402_types::scheme::{
    X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError, X402SchemeId,
};
use x402_types::util::Base64Bytes;

use crate::chain::ChainProvider;
use crate::chain::solana::{
    Address, SolanaChainProviderError, SolanaChainProviderLike, SolanaTokenDeployment,
};

pub use types::*;

pub const ATA_PROGRAM_PUBKEY: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

pub struct V1SolanaExact;

impl V1SolanaExact {
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn price_tag<A: Into<Address>>(
        pay_to: A,
        asset: DeployedTokenAmount<u64, SolanaTokenDeployment>,
    ) -> v1::PriceTag {
        let chain_id: ChainId = asset.token.chain_reference.into();
        let network = chain_id
            .as_network_name()
            .unwrap_or_else(|| panic!("Can not get network name for chain id {}", chain_id));
        v1::PriceTag {
            scheme: ExactScheme.to_string(),
            pay_to: pay_to.into().to_string(),
            asset: asset.token.address.to_string(),
            network: network.to_string(),
            amount: asset.amount.to_string(),
            max_timeout_seconds: 300,
            extra: None,
            enricher: Some(Arc::new(solana_fee_payer_enricher)),
        }
    }
}

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

impl X402SchemeFacilitatorBuilder<&ChainProvider> for V1SolanaExact {
    fn build(
        &self,
        provider: &ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let solana_provider = if let ChainProvider::Solana(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V1SolanaExact::build: provider must be a SolanaChainProvider".into());
        };
        self.build(solana_provider, config)
    }
}

impl<P> X402SchemeFacilitatorBuilder<P> for V1SolanaExact
where
    P: SolanaChainProviderLike + ChainProviderOps + Send + Sync + 'static,
{
    fn build(
        &self,
        provider: P,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let config = config
            .map(serde_json::from_value::<V1SolanaExactFacilitatorConfig>)
            .transpose()?
            .unwrap_or_default();

        Ok(Box::new(V1SolanaExactFacilitator::new(provider, config)))
    }
}

pub struct V1SolanaExactFacilitator<P> {
    provider: P,
    config: V1SolanaExactFacilitatorConfig,
}

impl<P> V1SolanaExactFacilitator<P> {
    pub fn new(provider: P, config: V1SolanaExactFacilitatorConfig) -> Self {
        Self { provider, config }
    }
}

#[async_trait::async_trait]
impl<P> X402SchemeFacilitator for V1SolanaExactFacilitator<P>
where
    P: SolanaChainProviderLike + ChainProviderOps + Send + Sync,
{
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError> {
        let request = types::VerifyRequest::from_proto(request.clone())?;
        let verification = verify_transfer(&self.provider, &request, &self.config).await?;
        Ok(v1::VerifyResponse::valid(verification.payer.to_string()).into())
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, X402SchemeFacilitatorError> {
        let request = types::SettleRequest::from_proto(request.clone())?;
        let verification = verify_transfer(&self.provider, &request, &self.config).await?;
        let payer = verification.payer.to_string();
        let tx_sig = settle_transaction(&self.provider, verification).await?;
        Ok(v1::SettleResponse::Success {
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
    pub mint: Pubkey,
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

/// Validates the instruction structure of the transaction.
///
/// Required structure:
/// - Index 0: SetComputeUnitLimit instruction
/// - Index 1: SetComputeUnitPrice instruction
/// - Index 2: TransferChecked instruction (Token or Token-2022)
/// - Index 3+: Additional instructions (only if allow_additional_instructions is true)
///
/// NOTE: CreateATA is NOT supported. The destination ATA must exist before payment.
pub fn validate_instructions(
    transaction: &VersionedTransaction,
    config: &V1SolanaExactFacilitatorConfig,
) -> Result<(), SolanaExactError> {
    let instructions = transaction.message.instructions();

    // Minimum: ComputeLimit + ComputePrice + TransferChecked
    if instructions.len() < 3 {
        return Err(SolanaExactError::TooFewInstructions);
    }

    // Check maximum instruction count
    if instructions.len() > config.max_instruction_count {
        return Err(SolanaExactError::InstructionCountExceedsMax(
            config.max_instruction_count,
        ));
    }

    // Verify instruction at index 2 is a token transfer (NOT CreateATA)
    let ix2_program = get_program_id(transaction, 2);
    if ix2_program == Some(ATA_PROGRAM_PUBKEY) {
        return Err(SolanaExactError::CreateATANotSupported);
    }

    // Validate additional instructions (if any beyond the required 3)
    if instructions.len() > 3 {
        if !config.allow_additional_instructions {
            return Err(SolanaExactError::AdditionalInstructionsNotAllowed);
        }

        // Validate each additional instruction (starting at index 3)
        for i in 3..instructions.len() {
            if let Some(program_id) = get_program_id(transaction, i) {
                // Check blocked list first (takes precedence)
                if config.is_blocked(&program_id) {
                    return Err(SolanaExactError::BlockedProgram(program_id));
                }

                // Check allowed list - must be explicitly whitelisted
                if !config.is_allowed(&program_id) {
                    return Err(SolanaExactError::ProgramNotAllowed(program_id));
                }
            }
        }
    }

    Ok(())
}

fn get_program_id(transaction: &VersionedTransaction, index: usize) -> Option<Pubkey> {
    let instruction = transaction.message.instructions().get(index)?;
    let account_keys = transaction.message.static_account_keys();
    Some(*instruction.program_id(account_keys))
}

pub async fn verify_transfer<P: SolanaChainProviderLike + ChainProviderOps>(
    provider: &P,
    request: &types::VerifyRequest,
    config: &V1SolanaExactFacilitatorConfig,
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
    let result = verify_transaction(
        provider,
        transaction_b64_string,
        &transfer_requirement,
        config,
    )
    .await?;
    Ok(result)
}

pub async fn verify_transaction<P: SolanaChainProviderLike>(
    provider: &P,
    transaction_b64_string: String,
    transfer_requirement: &TransferRequirement<'_>,
    config: &V1SolanaExactFacilitatorConfig,
) -> Result<VerifyTransferResult, PaymentVerificationError> {
    let bytes = Base64Bytes::from(transaction_b64_string.as_bytes())
        .decode()
        .map_err(|e| SolanaExactError::TransactionDecoding(e.to_string()))?;
    let transaction = bincode::deserialize::<VersionedTransaction>(bytes.as_slice())
        .map_err(|e| SolanaExactError::TransactionDecoding(e.to_string()))?;

    // Verify compute instructions
    let compute_units = verify_compute_limit_instruction(&transaction, 0)?;
    if compute_units > provider.max_compute_unit_limit() {
        return Err(SolanaExactError::MaxComputeUnitLimitExceeded.into());
    }
    tracing::debug!(compute_units = compute_units, "Verified compute unit limit");
    verify_compute_price_instruction(provider.max_compute_unit_price(), &transaction, 1)?;

    // Flexible instruction validation (replaces old instruction count check)
    validate_instructions(&transaction, config)?;

    // Transfer instruction is ALWAYS at index 2 (CreateATA no longer supported)
    let transfer_instruction =
        verify_transfer_instruction(provider, &transaction, 2, transfer_requirement).await?;

    // Fee payer safety check (configurable but defaults to enabled)
    if config.require_fee_payer_not_in_instructions {
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
    }

    // Sign and simulate transaction
    let tx = TransactionInt::new(transaction.clone()).sign(provider)?;
    let cfg = RpcSimulateTransactionConfig {
        sig_verify: false,
        replace_recent_blockhash: false,
        commitment: Some(CommitmentConfig::confirmed()),
        encoding: None,
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

pub async fn verify_transfer_instruction<P: SolanaChainProviderLike>(
    provider: &P,
    transaction: &VersionedTransaction,
    instruction_index: usize,
    transfer_requirement: &TransferRequirement<'_>,
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
        let mint = instruction.account(1)?;
        // Destination = 2
        let destination = instruction.account(2)?;
        // Authority = 3
        let authority = instruction.account(3)?;
        TransferCheckedInstruction {
            amount,
            source,
            mint,
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
        let mint = instruction.account(1)?;
        // Destination = 2
        let destination = instruction.account(2)?;
        // Authority = 3
        let authority = instruction.account(3)?;
        TransferCheckedInstruction {
            amount,
            source,
            mint,
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

    // Verify that the mint matches the expected asset
    if Address::new(transfer_checked_instruction.mint) != *transfer_requirement.asset {
        return Err(PaymentVerificationError::AssetMismatch);
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
    // Destination ATA must exist (CreateATA no longer supported)
    let is_receiver_missing = accounts.get(1).cloned().is_none_or(|a| a.is_none());
    if is_receiver_missing {
        return Err(PaymentVerificationError::RecipientMismatch);
    }
    let instruction_amount = transfer_checked_instruction.amount;
    if instruction_amount != transfer_requirement.amount {
        return Err(PaymentVerificationError::InvalidPaymentAmount);
    }
    Ok(transfer_checked_instruction)
}

pub async fn settle_transaction<P: SolanaChainProviderLike>(
    provider: &P,
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

impl From<SolanaChainProviderError> for PaymentVerificationError {
    fn from(value: SolanaChainProviderError) -> Self {
        Self::TransactionSimulation(value.to_string())
    }
}

/// Enricher function for Solana price tags - adds fee_payer to extra field
#[allow(dead_code)]
pub fn solana_fee_payer_enricher(
    price_tag: &mut v1::PriceTag,
    capabilities: &proto::SupportedResponse,
) {
    if price_tag.extra.is_some() {
        return;
    }

    // Find the matching kind and deserialize the whole extra into SupportedPaymentKindExtra
    let extra = capabilities
        .kinds
        .iter()
        .find(|kind| {
            v1::X402Version1 == kind.x402_version
                && kind.scheme == ExactScheme.to_string()
                && kind.network == price_tag.network
        })
        .and_then(|kind| kind.extra.as_ref())
        .and_then(|extra| serde_json::from_value::<SupportedPaymentKindExtra>(extra.clone()).ok());

    // Serialize the whole extra back to Value
    if let Some(extra) = extra {
        price_tag.extra = serde_json::to_value(&extra).ok();
    }
}
