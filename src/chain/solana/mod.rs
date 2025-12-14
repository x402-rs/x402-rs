use serde::{Deserialize, Deserializer, Serialize, Serializer};
use solana_client::nonblocking::pubsub_client::PubsubClient;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::pubsub_client::PubsubClientError;
use solana_client::rpc_client::SerializableTransaction;
use solana_client::rpc_config::{
    RpcSendTransactionConfig, RpcSignatureSubscribeConfig, RpcSimulateTransactionConfig,
};
use solana_client::rpc_response::RpcSignatureResult;
use solana_commitment_config::CommitmentConfig;
use solana_compute_budget_interface::ID as ComputeBudgetInstructionId;
use solana_keypair::Keypair;
use solana_message::compiled_instruction::CompiledInstruction;
use solana_pubkey::{Pubkey, pubkey};
use solana_signature::Signature;
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;
use std::collections::HashMap;
use std::fmt::{Debug, Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;
use tracing_core::Level;

use crate::chain::chain_id::{ChainId, ChainIdError};
use crate::chain::{FacilitatorLocalError, Namespace, NetworkProviderOps};
use crate::config::SolanaChainConfig;
use crate::facilitator::Facilitator;
use crate::network::Network;
use crate::proto::v1::X402Version1;
use crate::proto::v2::X402Version2;
use crate::types::{
    Base64Bytes, ExactPaymentPayload, FacilitatorErrorReason, MixedAddress, PaymentRequirements,
    Scheme, SettleRequest, SettleResponse, SupportedPaymentKind, SupportedPaymentKindExtra,
    SupportedResponse, TokenAmount, TransactionHash, VerifyRequest, VerifyResponse,
};

pub type Address = Pubkey; // TODO Maybe use solana_address

const ATA_PROGRAM_PUBKEY: Pubkey = pubkey!("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL");

/// A Solana chain reference consisting of 32 ASCII characters.
/// The genesis hash is the first 32 characters of the base58-encoded genesis block hash.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SolanaChainReference([u8; 32]);

impl SolanaChainReference {
    /// Creates a new SolanaChainReference from a 32-byte array.
    /// Returns None if any byte is not a valid ASCII character.
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the underlying bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns the chain reference as a string.
    pub fn as_str(&self) -> &str {
        // Safe because we validate ASCII on construction
        std::str::from_utf8(&self.0).expect("SolanaChainReference contains valid ASCII")
    }
}

/// Error type for parsing a SolanaChainReference from a string.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SolanaChainReferenceParseError {
    #[error("invalid length: expected 32 characters, got {0}")]
    InvalidLength(usize),
    #[error("string contains non-ASCII characters")]
    NonAscii,
}

impl FromStr for SolanaChainReference {
    type Err = SolanaChainReferenceParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.len() != 32 {
            return Err(SolanaChainReferenceParseError::InvalidLength(s.len()));
        }
        if !s.is_ascii() {
            return Err(SolanaChainReferenceParseError::NonAscii);
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(s.as_bytes());
        Ok(Self(bytes))
    }
}

impl Display for SolanaChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for SolanaChainReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SolanaChainReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl TryFrom<Network> for SolanaChainReference {
    type Error = FacilitatorLocalError;

    fn try_from(value: Network) -> Result<Self, Self::Error> {
        match value {
            Network::Solana => Ok(Self(*b"5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp")),
            Network::SolanaDevnet => Ok(Self(*b"EtWTRABZaYq6iMfeYKouRu166VU2xqa1")),
            Network::BaseSepolia => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
            Network::Base => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
            Network::XdcMainnet => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
            Network::AvalancheFuji => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
            Network::Avalanche => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
            Network::XrplEvm => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
            Network::PolygonAmoy => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
            Network::Polygon => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
            Network::Sei => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
            Network::SeiTestnet => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
        }
    }
}

impl Into<ChainId> for SolanaChainReference {
    fn into(self) -> ChainId {
        ChainId::solana(self.as_str())
    }
}

impl TryFrom<ChainId> for SolanaChainReference {
    type Error = ChainIdError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        if value.namespace != Namespace::Solana.to_string() {
            return Err(ChainIdError::UnexpectedNamespace(
                value.namespace,
                Namespace::Solana,
            ));
        }
        let solana_chain_reference = Self::from_str(&value.reference).map_err(|e| {
            ChainIdError::InvalidReference(value.reference, Namespace::Solana, format!("{e:?}"))
        })?;
        Ok(solana_chain_reference)
    }
}

#[derive(Clone, Debug)]
pub struct SolanaAddress {
    pubkey: Pubkey,
}

impl From<Pubkey> for SolanaAddress {
    fn from(pubkey: Pubkey) -> Self {
        Self { pubkey }
    }
}

impl From<SolanaAddress> for Pubkey {
    fn from(address: SolanaAddress) -> Self {
        address.pubkey
    }
}

impl TryFrom<MixedAddress> for SolanaAddress {
    type Error = FacilitatorLocalError;

    fn try_from(value: MixedAddress) -> Result<Self, Self::Error> {
        match value {
            MixedAddress::Evm(_) | MixedAddress::Offchain(_) => Err(
                FacilitatorLocalError::InvalidAddress("expected Solana address".to_string()),
            ),
            MixedAddress::Solana(pubkey) => Ok(Self { pubkey }),
        }
    }
}

impl From<SolanaAddress> for MixedAddress {
    fn from(value: SolanaAddress) -> Self {
        MixedAddress::Solana(value.pubkey)
    }
}

#[derive(Clone)]
pub struct SolanaProvider {
    keypair: Arc<Keypair>,
    chain: SolanaChainReference,
    rpc_client: Arc<RpcClient>,
    pubsub_client: Arc<Option<PubsubClient>>,
    max_compute_unit_limit: u32,
    max_compute_unit_price: u64,
}

impl Debug for SolanaProvider {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SolanaProvider")
            .field("pubkey", &self.keypair.pubkey())
            .field("chain", &self.chain)
            .field("rpc_url", &self.rpc_client.url())
            .finish()
    }
}

impl SolanaProvider {
    pub async fn from_config(config: &SolanaChainConfig) -> Result<Self, PubsubClientError> {
        let rpc_url = config.rpc();
        let pubsub_url = config.pubsub().clone().map(|url| url.to_string());
        let keypair = Keypair::from_base58_string(&config.signer().to_string());
        let max_compute_unit_limit = config.max_compute_unit_limit();
        let max_compute_unit_price = config.max_compute_unit_price();
        let chain = config.chain_reference();
        SolanaProvider::new(
            keypair,
            rpc_url.to_string(),
            pubsub_url,
            chain,
            max_compute_unit_limit,
            max_compute_unit_price,
        )
        .await
    }

    pub async fn new(
        keypair: Keypair,
        rpc_url: String,
        pubsub_url: Option<String>,
        chain: SolanaChainReference,
        max_compute_unit_limit: u32,
        max_compute_unit_price: u64,
    ) -> Result<Self, PubsubClientError> {
        {
            let signer_addresses = vec![keypair.pubkey()];
            let chain_id: ChainId = chain.into();
            tracing::info!(
                chain = %chain_id,
                rpc = rpc_url,
                pubsub = ?pubsub_url,
                signers = ?signer_addresses,
                max_compute_unit_limit,
                max_compute_unit_price,
                "Initialized Solana provider"
            );
        }
        let rpc_client = RpcClient::new(rpc_url);
        let pubsub_client = if let Some(pubsub_url) = pubsub_url {
            let client = PubsubClient::new(pubsub_url).await?;
            Some(client)
        } else {
            None
        };
        Ok(Self {
            keypair: Arc::new(keypair),
            chain,
            rpc_client: Arc::new(rpc_client),
            pubsub_client: Arc::new(pubsub_client),
            max_compute_unit_limit,
            max_compute_unit_price,
        })
    }

    pub fn verify_compute_limit_instruction(
        &self,
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
        &self,
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
        if compute_budget.ne(account) || data.first().cloned().unwrap_or(0) != 3 || data.len() != 9
        {
            return Err(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_instructions_compute_price_instruction"
                    .to_string(),
            ));
        }
        // It is ComputeBudgetInstruction definitely by now!
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&data[1..]);
        let microlamports = u64::from_le_bytes(buf);
        if microlamports > self.max_compute_unit_price {
            return Err(FacilitatorLocalError::DecodingError(
                "compute unit price exceeds facilitator maximum".to_string(),
            ));
        }
        Ok(())
    }

    pub fn verify_create_ata_instruction(
        &self,
        transaction: &VersionedTransaction,
        index: usize,
        requirements: &PaymentRequirements,
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
        let pay_to: SolanaAddress = requirements.pay_to.clone().try_into()?;
        if owner != pay_to.into() {
            return Err(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_create_ata_instruction_incorrect_payee"
                    .to_string(),
            ));
        }
        let asset: SolanaAddress = requirements.asset.clone().try_into()?;
        if mint != asset.into() {
            return Err(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_create_ata_instruction_incorrect_asset"
                    .to_string(),
            ));
        }

        Ok(())
    }

    // this expects the destination ATA to already exist
    pub async fn verify_transfer_instruction(
        &self,
        transaction: &VersionedTransaction,
        instruction_index: usize,
        requirements: &PaymentRequirements,
        has_dest_ata: bool,
    ) -> Result<TransferCheckedInstruction, FacilitatorLocalError> {
        let tx = TransactionInt::new(transaction.clone());
        let instruction = tx.instruction(instruction_index)?;
        instruction.assert_not_empty()?;
        let program_id = instruction.program_id();
        let transfer_checked_instruction = if spl_token::ID.eq(&program_id) {
            let token_instruction =
                spl_token::instruction::TokenInstruction::unpack(instruction.data_slice())
                    .map_err(|_| {
                        FacilitatorLocalError::DecodingError(
                            "invalid_exact_svm_payload_transaction_instructions".to_string(),
                        )
                    })?;
            let (amount, decimals) = match token_instruction {
                spl_token::instruction::TokenInstruction::TransferChecked { amount, decimals } => {
                    (amount, decimals)
                }
                _ => {
                    return Err(FacilitatorLocalError::DecodingError(
                        "invalid_exact_svm_payload_transaction_instructions".to_string(),
                    ));
                }
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
                decimals,
                source,
                mint,
                destination,
                authority,
                token_program: spl_token::ID,
                data: instruction.data(),
            }
        } else if spl_token_2022::ID.eq(&program_id) {
            let token_instruction =
                spl_token_2022::instruction::TokenInstruction::unpack(instruction.data_slice())
                    .map_err(|_| {
                        FacilitatorLocalError::DecodingError(
                            "invalid_exact_svm_payload_transaction_instructions".to_string(),
                        )
                    })?;
            let (amount, decimals) = match token_instruction {
                spl_token_2022::instruction::TokenInstruction::TransferChecked {
                    amount,
                    decimals,
                } => (amount, decimals),
                _ => {
                    return Err(FacilitatorLocalError::DecodingError(
                        "invalid_exact_svm_payload_transaction_instructions".to_string(),
                    ));
                }
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
                decimals,
                source,
                mint,
                destination,
                authority,
                token_program: spl_token_2022::ID,
                data: instruction.data(),
            }
        } else {
            return Err(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_not_a_transfer_instruction".to_string(),
            ));
        };

        // Verify that the fee payer is not transferring funds (not the authority)
        let fee_payer_pubkey = self.keypair.pubkey();
        if transfer_checked_instruction.authority == fee_payer_pubkey {
            return Err(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_fee_payer_transferring_funds".to_string(),
            ));
        }

        let asset_address: SolanaAddress = requirements.asset.clone().try_into()?;
        let pay_to_address: SolanaAddress = requirements.pay_to.clone().try_into()?;
        let token_program = transfer_checked_instruction.token_program;
        // findAssociatedTokenPda
        let (ata, _) = Pubkey::find_program_address(
            &[
                pay_to_address.pubkey.as_ref(),
                token_program.as_ref(),
                asset_address.pubkey.as_ref(),
            ],
            &ATA_PROGRAM_PUBKEY,
        );
        if transfer_checked_instruction.destination != ata {
            return Err(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_transfer_to_incorrect_ata".to_string(),
            ));
        }
        let accounts = self
            .rpc_client
            .get_multiple_accounts(&[transfer_checked_instruction.source, ata])
            .await
            .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e}")))?;
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
        let instruction_amount: TokenAmount = transfer_checked_instruction.amount.into();
        let requirements_amount: TokenAmount = requirements.max_amount_required;
        if instruction_amount != requirements_amount {
            return Err(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_amount_mismatch".to_string(),
            ));
        }
        Ok(transfer_checked_instruction)
    }

    async fn verify_transfer(
        &self,
        request: &VerifyRequest,
    ) -> Result<VerifyTransferResult, FacilitatorLocalError> {
        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;

        // Assert valid payment START
        let payment_payload = match &payload.payload {
            ExactPaymentPayload::Evm(..) => {
                return Err(FacilitatorLocalError::UnsupportedNetwork(None));
            }
            ExactPaymentPayload::Solana(payload) => payload,
        };
        if payload.network.as_chain_id() != self.chain_id() {
            return Err(FacilitatorLocalError::NetworkMismatch(
                None,
                self.chain_id(),
                payload.network.as_chain_id(),
            ));
        }
        if requirements.network.as_chain_id() != self.chain_id() {
            return Err(FacilitatorLocalError::NetworkMismatch(
                None,
                self.chain_id(),
                requirements.network.as_chain_id(),
            ));
        }
        if payload.scheme != requirements.scheme {
            return Err(FacilitatorLocalError::SchemeMismatch(
                None,
                requirements.scheme,
                payload.scheme,
            ));
        }
        let transaction_b64_string = payment_payload.transaction.clone();
        let bytes = Base64Bytes::from(transaction_b64_string.as_bytes())
            .decode()
            .map_err(|e| FacilitatorLocalError::DecodingError(format!("{e}")))?;
        let transaction = bincode::deserialize::<VersionedTransaction>(bytes.as_slice())
            .map_err(|e| FacilitatorLocalError::DecodingError(format!("{e}")))?;

        // perform transaction introspection to validate the transaction structure and details
        let instructions = transaction.message.instructions();
        let compute_units = self.verify_compute_limit_instruction(&transaction, 0)?;
        if compute_units > self.max_compute_unit_limit {
            return Err(FacilitatorLocalError::DecodingError(
                "compute unit limit exceeds facilitator maximum".to_string(),
            ));
        }
        tracing::debug!(compute_units = compute_units, "Verified compute unit limit");
        self.verify_compute_price_instruction(&transaction, 1)?;
        let transfer_instruction = if instructions.len() == 3 {
            // verify that the transfer instruction is valid
            // this expects the destination ATA to already exist
            self.verify_transfer_instruction(&transaction, 2, requirements, false)
                .await?
        } else if instructions.len() == 4 {
            // verify that the transfer instruction is valid
            // this expects the destination ATA to be created in the same transaction
            self.verify_create_ata_instruction(&transaction, 2, requirements)?;
            self.verify_transfer_instruction(&transaction, 3, requirements, true)
                .await?
        } else {
            return Err(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_instructions_count".to_string(),
            ));
        };

        // Rule 2: Fee payer safety check
        // Verify that the fee payer is not included in any instruction's accounts
        // This single check covers all cases: authority, source, or any other role
        let fee_payer_pubkey = self.keypair.pubkey();
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

        let tx = TransactionInt::new(transaction.clone()).sign(&self.keypair)?;
        let cfg = RpcSimulateTransactionConfig {
            sig_verify: false,
            replace_recent_blockhash: false,
            commitment: Some(CommitmentConfig::confirmed()),
            encoding: None, // optional; client handles encoding
            accounts: None,
            inner_instructions: false,
            min_context_slot: None,
        };
        let sim = self
            .rpc_client
            .simulate_transaction_with_config(&tx.inner, cfg)
            .await
            .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e}")))?;
        if sim.value.err.is_some() {
            return Err(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_simulation_failed".to_string(),
            ));
        }
        let payer: SolanaAddress = transfer_instruction.authority.into();
        Ok(VerifyTransferResult { payer, transaction })
    }

    pub fn fee_payer(&self) -> MixedAddress {
        let pubkey = self.keypair.pubkey();
        MixedAddress::Solana(pubkey)
    }
}

pub struct VerifyTransferResult {
    pub payer: SolanaAddress,
    pub transaction: VersionedTransaction,
}

#[derive(Debug)]
pub struct TransferCheckedInstruction {
    pub amount: u64,
    pub decimals: u8,
    pub source: Pubkey,
    pub mint: Pubkey,
    pub destination: Pubkey,
    pub authority: Pubkey,
    pub token_program: Pubkey,
    pub data: Vec<u8>,
}

impl NetworkProviderOps for SolanaProvider {
    fn signer_addresses(&self) -> Vec<MixedAddress> {
        vec![self.fee_payer()]
    }

    fn chain_id(&self) -> ChainId {
        ChainId::solana(self.chain.as_str())
    }
}

impl Facilitator for SolanaProvider {
    type Error = FacilitatorLocalError;

    async fn verify(&self, request: &VerifyRequest) -> Result<VerifyResponse, Self::Error> {
        let verification = self.verify_transfer(request).await?;
        Ok(VerifyResponse::valid(verification.payer.into()))
    }

    async fn settle(&self, request: &SettleRequest) -> Result<SettleResponse, Self::Error> {
        let verification = self.verify_transfer(request).await?;
        let tx = TransactionInt::new(verification.transaction).sign(&self.keypair)?;
        // Verify if fully signed
        if !tx.is_fully_signed() {
            tracing::event!(Level::WARN, status = "failed", "undersigned transaction");
            return Ok(SettleResponse {
                success: false,
                error_reason: Some(FacilitatorErrorReason::UnexpectedSettleError),
                payer: verification.payer.into(),
                transaction: None,
                network: self
                    .chain_id()
                    .try_into()
                    .map_err(FacilitatorLocalError::NetworkConversionError)?,
            });
        }
        let tx_sig = tx
            .send_and_confirm(
                &self.rpc_client,
                &self.pubsub_client,
                CommitmentConfig::confirmed(),
            )
            .await?;
        let settle_response = SettleResponse {
            success: true,
            error_reason: None,
            payer: verification.payer.into(),
            transaction: Some(TransactionHash::Solana(*tx_sig.as_array())),
            network: self
                .chain_id()
                .try_into()
                .map_err(FacilitatorLocalError::NetworkConversionError)?,
        };
        Ok(settle_response)
    }

    async fn supported(&self) -> Result<SupportedResponse, Self::Error> {
        let kinds = {
            let mut kinds = Vec::with_capacity(2);
            let extra = self
                .signer_addresses()
                .first()
                .map(|address| SupportedPaymentKindExtra {
                    fee_payer: address.clone(),
                });
            match extra {
                None => kinds,
                Some(extra) => {
                    let network: Option<Network> = self.chain_id().try_into().ok();
                    if let Some(network) = network {
                        kinds.push(SupportedPaymentKind::V1 {
                            x402_version: X402Version1,
                            scheme: Scheme::Exact,
                            network: network.to_string(),
                            extra: Some(extra.clone()),
                        });
                    }
                    kinds.push(SupportedPaymentKind::V2 {
                        x402_version: X402Version2,
                        scheme: Scheme::Exact,
                        network: self.chain_id(),
                        extra: Some(extra),
                    });
                    kinds
                }
            }
        };
        let signers = {
            let mut signers = HashMap::with_capacity(1);
            signers.insert(self.chain_id(), self.signer_addresses());
            signers
        };
        Ok(SupportedResponse {
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

    pub fn data(&self) -> Vec<u8> {
        self.instruction.data.clone()
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

    pub fn sign(self, keypair: &Keypair) -> Result<Self, FacilitatorLocalError> {
        let mut tx = self.inner.clone();
        let msg_bytes = tx.message.serialize();
        let signature = keypair
            .try_sign_message(msg_bytes.as_slice())
            .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e}")))?;
        // Required signatures are the first N account keys
        let num_required = tx.message.header().num_required_signatures as usize;
        let static_keys = tx.message.static_account_keys();
        // Find signer’s position
        let pos = static_keys[..num_required]
            .iter()
            .position(|k| *k == keypair.pubkey())
            .ok_or(FacilitatorLocalError::DecodingError(
                "invalid_exact_svm_payload_transaction_simulation_failed".to_string(),
            ))?;
        // Ensure signature vector is large enough, then place the signature
        if tx.signatures.len() < num_required {
            tx.signatures.resize(num_required, Signature::default());
        }
        // tx.signatures.push(signature);
        tx.signatures[pos] = signature;
        Ok(Self { inner: tx })
    }

    pub async fn send(&self, rpc_client: &RpcClient) -> Result<Signature, FacilitatorLocalError> {
        rpc_client
            .send_transaction_with_config(
                &self.inner,
                RpcSendTransactionConfig {
                    skip_preflight: true,
                    ..RpcSendTransactionConfig::default()
                },
            )
            .await
            .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e}")))
    }

    pub async fn send_and_confirm(
        &self,
        rpc_client: &RpcClient,
        pubsub_client: &Option<PubsubClient>,
        commitment_config: CommitmentConfig,
    ) -> Result<Signature, FacilitatorLocalError> {
        let tx_sig = self.inner.get_signature();

        use futures_util::stream::StreamExt;

        if let Some(pubsub_client) = pubsub_client {
            let config = RpcSignatureSubscribeConfig {
                commitment: Some(commitment_config),
                enable_received_notification: None,
            };
            let (mut stream, unsubscribe) = pubsub_client
                .signature_subscribe(tx_sig, Some(config))
                .await
                .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e}")))?;
            if let Err(e) = self.send(rpc_client).await {
                tracing::error!(error = %e, "Failed to send transaction");
                unsubscribe().await;
                return Err(e);
            }
            while let Some(response) = stream.next().await {
                let error = if let RpcSignatureResult::ProcessedSignature(r) = response.value {
                    r.err
                } else {
                    None
                };
                return match error {
                    None => Ok(tx_sig.clone()),
                    Some(error) => Err(FacilitatorLocalError::ContractCall(format!("{error}"))),
                };
            }
            Err(FacilitatorLocalError::DecodingError(
                "signature_subscribe error".to_string(),
            ))
        } else {
            self.send(rpc_client).await?;
            loop {
                let confirmed = rpc_client
                    .confirm_transaction_with_commitment(tx_sig, commitment_config)
                    .await
                    .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e}")))?;
                if confirmed.value {
                    return Ok(tx_sig.clone());
                }
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
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
