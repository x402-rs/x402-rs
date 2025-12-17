pub mod types;

use serde::{Deserialize, Serialize};
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_commitment_config::CommitmentConfig;
use solana_compute_budget_interface::ID as ComputeBudgetInstructionId;
use solana_message::compiled_instruction::CompiledInstruction;
use solana_pubkey::{Pubkey, pubkey};
use solana_signature::Signature;
use solana_transaction::versioned::VersionedTransaction;
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tracing_core::Level;

use crate::chain::solana::{Address, SolanaChainProvider};
use crate::chain::{ChainId, ChainProvider, ChainProviderOps};
use crate::facilitator_local::FacilitatorLocalError;
use crate::proto;
use crate::scheme::v1_eip155_exact::EXACT_SCHEME;
use crate::scheme::v1_solana_exact::types::SupportedPaymentKindExtra;
use crate::scheme::{SchemeSlug, X402SchemeBlueprint, X402SchemeHandler};
use crate::util::Base64Bytes;

pub const ATA_PROGRAM_PUBKEY: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

pub struct V1SolanaExact;

impl X402SchemeBlueprint for V1SolanaExact {
    fn slug(&self) -> SchemeSlug {
        SchemeSlug::new(1, "solana", EXACT_SCHEME.to_string())
    }

    // FIXME Pass json config value
    fn build(&self, provider: ChainProvider) -> Result<Box<dyn X402SchemeHandler>, Box<dyn Error>> {
        let provider = if let ChainProvider::Solana(provider) = provider {
            provider
        } else {
            return Err("V1SolanaExact::build: provider must be a SolanaChainProvider".into());
        };
        Ok(Box::new(V1SolanaExactHandler { provider }))
    }
}

pub struct V1SolanaExactHandler {
    provider: Arc<SolanaChainProvider>,
}

#[async_trait::async_trait]
impl X402SchemeHandler for V1SolanaExactHandler {
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, FacilitatorLocalError> {
        let request = types::VerifyRequest::from_proto(request.clone()).ok_or(
            FacilitatorLocalError::DecodingError("Can not decode payload".to_string()),
        )?;
        let verification = verify_transfer(&self.provider, request).await?;
        Ok(proto::v1::VerifyResponse::valid(verification.payer.to_string()).into())
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, FacilitatorLocalError> {
        let request = types::SettleRequest::from_proto(request.clone()).ok_or(
            FacilitatorLocalError::DecodingError("Can not decode payload".to_string()),
        )?;
        let verification = verify_transfer(&self.provider, request).await?;
        let tx = TransactionInt::new(verification.transaction).sign(&self.provider)?;
        // Verify if fully signed
        if !tx.is_fully_signed() {
            tracing::event!(Level::WARN, status = "failed", "undersigned transaction");
            return Ok(proto::v1::SettleResponse::Error {
                reason: "unexpected_settle_error".to_string(),
                network: self.provider.chain_id().to_string(),
            }
            .into());
        }
        let tx_sig = tx
            .send_and_confirm(&self.provider, CommitmentConfig::confirmed())
            .await?;
        Ok(proto::v1::SettleResponse::Success {
            payer: verification.payer.to_string(),
            transaction: tx_sig.to_string(),
            network: self.provider.chain_id().to_string(),
        }
        .into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, FacilitatorLocalError> {
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
                    scheme: EXACT_SCHEME.to_string(),
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

    pub fn assert_not_empty(&self) -> Result<(), FacilitatorLocalError> {
        if !self.has_data() || !self.has_accounts() {
            return Err(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_instructions".to_string(),
            ));
        }
        Ok(())
    }

    pub fn program_id(&self) -> Pubkey {
        *self.instruction.program_id(self.account_keys.as_slice())
    }

    pub fn account(&self, index: usize) -> Result<Pubkey, FacilitatorLocalError> {
        let account_index = self.instruction.accounts.get(index).cloned().ok_or(
            FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_instructions".to_string(),
            ),
        )?;
        let pubkey = self
            .account_keys
            .get(account_index as usize)
            .cloned()
            .ok_or(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_instructions".to_string(),
            ))?;
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
    pub fn instruction(&self, index: usize) -> Result<InstructionInt, FacilitatorLocalError> {
        let instruction = self
            .inner
            .message
            .instructions()
            .get(index)
            .cloned()
            .ok_or(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_instructions".to_string(),
            ))?;
        let account_keys = self.inner.message.static_account_keys().to_vec();

        Ok(InstructionInt {
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

    pub fn sign(self, provider: &SolanaChainProvider) -> Result<Self, FacilitatorLocalError> {
        let tx = provider.sign(self.inner)?;
        Ok(Self { inner: tx })
    }

    pub async fn send_and_confirm(
        &self,
        provider: &SolanaChainProvider,
        commitment_config: CommitmentConfig,
    ) -> Result<Signature, FacilitatorLocalError> {
        provider
            .send_and_confirm(&self.inner, commitment_config)
            .await
    }

    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn as_base64(&self) -> Result<String, FacilitatorLocalError> {
        let bytes = bincode::serialize(&self.inner)
            .map_err(|e| FacilitatorLocalError::DecodingError(format!("{e}")))?;
        let base64_bytes = Base64Bytes::encode(bytes);
        let string = String::from_utf8(base64_bytes.0.into_owned())
            .map_err(|e| FacilitatorLocalError::DecodingError(format!("{e}")))?;
        Ok(string)
    }
}

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
) -> Result<u32, FacilitatorLocalError> {
    let instructions = transaction.message.instructions();
    let instruction =
        instructions
            .get(instruction_index)
            .ok_or(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_instructions_length".to_string(),
            ))?;
    let account = instruction.program_id(transaction.message.static_account_keys());
    let data = instruction.data.as_slice();

    // Verify program ID, discriminator, and data length (1 byte discriminator + 4 bytes u32)
    if ComputeBudgetInstructionId.ne(account)
        || data.first().cloned().unwrap_or(0) != 2
        || data.len() != 5
    {
        return Err(FacilitatorLocalError::DecodingError(
            "invalid_exact_svm_payload_transaction_compute_limit_instruction".to_string(),
        ));
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
) -> Result<(), FacilitatorLocalError> {
    let instructions = transaction.message.instructions();
    let instruction =
        instructions
            .get(instruction_index)
            .ok_or(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_instructions_compute_price_instruction"
                    .to_string(),
            ))?;
    let account = instruction.program_id(transaction.message.static_account_keys());
    let compute_budget = solana_compute_budget_interface::ID;
    let data = instruction.data.as_slice();
    if compute_budget.ne(account) || data.first().cloned().unwrap_or(0) != 3 || data.len() != 9 {
        return Err(FacilitatorLocalError::DecodingError(
            "invalid_exact_svm_payload_transaction_instructions_compute_price_instruction"
                .to_string(),
        ));
    }
    // It is ComputeBudgetInstruction definitely by now!
    let mut buf = [0u8; 8];
    buf.copy_from_slice(&data[1..]);
    let microlamports = u64::from_le_bytes(buf);
    if microlamports > max_compute_unit_price {
        return Err(FacilitatorLocalError::DecodingError(
            "compute unit price exceeds facilitator maximum".to_string(),
        ));
    }
    Ok(())
}

pub fn verify_create_ata_instruction(
    transaction: &VersionedTransaction,
    index: usize,
    transfer_requirement: &TransferRequirement,
) -> Result<(), FacilitatorLocalError> {
    let tx = TransactionInt::new(transaction.clone());
    let instruction = tx.instruction(index)?;
    instruction.assert_not_empty()?;

    // Verify program ID is the Associated Token Account Program
    let program_id = instruction.program_id();
    if program_id != ATA_PROGRAM_PUBKEY {
        return Err(FacilitatorLocalError::DecodingError(
            "invalid_exact_svm_payload_transaction_create_ata_instruction".to_string(),
        ));
    }

    // Verify instruction discriminator
    // The ATA program's Create instruction has discriminator 0 (Create) or 1 (CreateIdempotent)
    let data = instruction.data_slice();
    if data.is_empty() || (data[0] != 0 && data[0] != 1) {
        return Err(FacilitatorLocalError::DecodingError(
            "invalid_exact_svm_payload_transaction_create_ata_instruction".to_string(),
        ));
    }

    // Verify account count (must have at least 6 accounts)
    if instruction.instruction.accounts.len() < 6 {
        return Err(FacilitatorLocalError::DecodingError(
            "invalid_exact_svm_payload_transaction_create_ata_instruction".to_string(),
        ));
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
        return Err(FacilitatorLocalError::DecodingError(
            "invalid_exact_svm_payload_transaction_create_ata_instruction_incorrect_payee"
                .to_string(),
        ));
    }
    if Address::new(mint) != *transfer_requirement.asset {
        return Err(FacilitatorLocalError::DecodingError(
            "invalid_exact_svm_payload_transaction_create_ata_instruction_incorrect_asset"
                .to_string(),
        ));
    }

    Ok(())
}

pub async fn verify_transfer(
    provider: &SolanaChainProvider,
    request: types::VerifyRequest,
) -> Result<VerifyTransferResult, FacilitatorLocalError> {
    let payload = &request.payment_payload;
    let requirements = &request.payment_requirements;

    // Assert valid payment START
    let chain_id = provider.chain_id();
    let payload_chain_id = ChainId::from_network_name(&payload.network)
        .ok_or(FacilitatorLocalError::UnsupportedNetwork(None))?;
    if payload_chain_id != chain_id {
        return Err(FacilitatorLocalError::NetworkMismatch(
            None,
            chain_id.to_string(),
            payload_chain_id.to_string(),
        ));
    }
    let requirements_chain_id = ChainId::from_network_name(&requirements.network)
        .ok_or(FacilitatorLocalError::UnsupportedNetwork(None))?;
    if requirements_chain_id != chain_id {
        return Err(FacilitatorLocalError::NetworkMismatch(
            None,
            chain_id.to_string(),
            requirements_chain_id.to_string(),
        ));
    }
    if payload.scheme != requirements.scheme {
        return Err(FacilitatorLocalError::SchemeMismatch(
            None,
            requirements.scheme.to_string(),
            payload.scheme.to_string(),
        ));
    }

    let transaction_b64_string = payload.payload.transaction.clone();
    let transfer_requirement = TransferRequirement {
        pay_to: &requirements.pay_to,
        asset: &requirements.asset,
        amount: requirements.max_amount_required.inner(),
    };
    verify_transaction(&provider, transaction_b64_string, &transfer_requirement).await
}

pub async fn verify_transaction(
    provider: &SolanaChainProvider,
    transaction_b64_string: String,
    transfer_requirement: &TransferRequirement<'_>,
) -> Result<VerifyTransferResult, FacilitatorLocalError> {
    let bytes = Base64Bytes::from(transaction_b64_string.as_bytes())
        .decode()
        .map_err(|e| FacilitatorLocalError::DecodingError(format!("{e}")))?;
    let transaction = bincode::deserialize::<VersionedTransaction>(bytes.as_slice())
        .map_err(|e| FacilitatorLocalError::DecodingError(format!("{e}")))?;

    // perform transaction introspection to validate the transaction structure and details
    let instructions = transaction.message.instructions();
    let compute_units = verify_compute_limit_instruction(&transaction, 0)?;
    if compute_units > provider.max_compute_unit_limit() {
        return Err(FacilitatorLocalError::DecodingError(
            "compute unit limit exceeds facilitator maximum".to_string(),
        ));
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
        return Err(FacilitatorLocalError::DecodingError(
            "invalid_exact_svm_payload_transaction_instructions_count".to_string(),
        ));
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
                .ok_or(FacilitatorLocalError::DecodingError(
                    "invalid_account_index".to_string(),
                ))?;

            if *account == fee_payer_pubkey {
                return Err(FacilitatorLocalError::DecodingError(
                        "invalid_exact_svm_payload_transaction_fee_payer_included_in_instruction_accounts".to_string(),
                    ));
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
) -> Result<TransferCheckedInstruction, FacilitatorLocalError> {
    let tx = TransactionInt::new(transaction.clone());
    let instruction = tx.instruction(instruction_index)?;
    instruction.assert_not_empty()?;
    let program_id = instruction.program_id();
    let transfer_checked_instruction = if spl_token::ID.eq(&program_id) {
        let token_instruction = spl_token::instruction::TokenInstruction::unpack(
            instruction.data_slice(),
        )
        .map_err(|_| {
            FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_instructions".to_string(),
            )
        })?;
        let amount = match token_instruction {
            spl_token::instruction::TokenInstruction::TransferChecked {
                amount,
                decimals: _,
            } => amount,
            _ => {
                return Err(FacilitatorLocalError::DecodingError(
                    "invalid_exact_svm_payload_transaction_instructions".to_string(),
                ));
            }
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
                .map_err(|_| {
                    FacilitatorLocalError::DecodingError(
                        "invalid_exact_svm_payload_transaction_instructions".to_string(),
                    )
                })?;
        let amount = match token_instruction {
            spl_token_2022::instruction::TokenInstruction::TransferChecked {
                amount,
                decimals: _,
            } => amount,
            _ => {
                return Err(FacilitatorLocalError::DecodingError(
                    "invalid_exact_svm_payload_transaction_instructions".to_string(),
                ));
            }
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
        return Err(FacilitatorLocalError::DecodingError(
            "invalid_exact_svm_payload_transaction_not_a_transfer_instruction".to_string(),
        ));
    };

    // Verify that the fee payer is not transferring funds (not the authority)
    let fee_payer_pubkey = provider.pubkey();
    if transfer_checked_instruction.authority == fee_payer_pubkey {
        return Err(FacilitatorLocalError::DecodingError(
            "invalid_exact_svm_payload_transaction_fee_payer_transferring_funds".to_string(),
        ));
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
        return Err(FacilitatorLocalError::DecodingError(
            "invalid_exact_svm_payload_transaction_transfer_to_incorrect_ata".to_string(),
        ));
    }
    let accounts = provider
        .get_multiple_accounts(&[transfer_checked_instruction.source, ata])
        .await?;
    let is_sender_missing = accounts.first().cloned().is_none_or(|a| a.is_none());
    if is_sender_missing {
        return Err(FacilitatorLocalError::DecodingError(
            "invalid_exact_svm_payload_transaction_sender_ata_not_found".to_string(),
        ));
    }
    let is_receiver_missing = accounts.get(1).cloned().is_none_or(|a| a.is_none());
    if is_receiver_missing && !has_dest_ata {
        return Err(FacilitatorLocalError::DecodingError(
            "invalid_exact_svm_payload_transaction_receiver_ata_not_found".to_string(),
        ));
    }
    let instruction_amount = transfer_checked_instruction.amount;
    if instruction_amount != transfer_requirement.amount {
        return Err(FacilitatorLocalError::DecodingError(
            "invalid_exact_svm_payload_transaction_amount_mismatch".to_string(),
        ));
    }
    Ok(transfer_checked_instruction)
}
