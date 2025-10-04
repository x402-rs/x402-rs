use async_trait::async_trait;
use solana_client::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_sdk::compute_budget::ComputeBudgetInstruction;
use solana_sdk::hash::Hash;
use solana_sdk::instruction::Instruction;
use solana_sdk::message::{VersionedMessage, v0::Message as MessageV0};
use solana_sdk::program_pack::Pack;
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{Keypair, Signature, Signer};
use solana_sdk::transaction::VersionedTransaction;
use spl_associated_token_account::instruction::create_associated_token_account_idempotent;
use std::str::FromStr;
use std::sync::Arc;
use x402_rs::chain::solana::{SolanaAddress, TransactionInt};
use x402_rs::network::NetworkFamily;
use x402_rs::types::{
    ExactPaymentPayload, ExactSolanaPayload, PaymentPayload, PaymentRequirements, X402Version,
};

use crate::X402PaymentsError;
use crate::chains::{IntoSenderWallet, SenderWallet};

#[derive(Clone)]
pub struct SolanaSenderWallet {
    keypair: Arc<Keypair>,
    rpc_client: Arc<RpcClient>,
}

impl SolanaSenderWallet {
    pub fn new(keypair: Keypair, rpc_client: RpcClient) -> Self {
        Self {
            keypair: Arc::new(keypair),
            rpc_client: Arc::new(rpc_client),
        }
    }

    fn fetch_mint(&self, mint_address: &SolanaAddress) -> Result<Mint, X402PaymentsError> {
        let mint_address: Pubkey = mint_address.clone().into();
        let account = self.rpc_client.get_account(&mint_address).map_err(|e| {
            X402PaymentsError::SigningError(format!("failed to fetch mint {mint_address}: {e}"))
        })?;
        if account.owner == spl_token::id() {
            let mint = spl_token::state::Mint::unpack(&account.data).map_err(|e| {
                X402PaymentsError::SigningError(format!(
                    "failed to unpack mint {mint_address}: {e}"
                ))
            })?;
            Ok(Mint::Token {
                decimals: mint.decimals,
                token_program: spl_token::id(),
            })
        } else if account.owner == spl_token_2022::id() {
            let mint = spl_token_2022::state::Mint::unpack(&account.data).map_err(|e| {
                X402PaymentsError::SigningError(format!(
                    "failed to unpack mint {mint_address}: {e}",
                ))
            })?;
            Ok(Mint::Token2022 {
                decimals: mint.decimals,
                token_program: spl_token_2022::id(),
            })
        } else {
            Err(X402PaymentsError::SigningError(format!(
                "failed to unpack mint {mint_address}: unknown owner"
            )))
        }
    }
}

impl IntoSenderWallet for SolanaSenderWallet {
    fn into_sender_wallet(self) -> Arc<dyn SenderWallet> {
        Arc::new(self)
    }
}

#[async_trait]
impl SenderWallet for SolanaSenderWallet {
    fn can_handle(&self, requirements: &PaymentRequirements) -> bool {
        let network = requirements.network;
        let network_family: NetworkFamily = network.into();
        match network_family {
            NetworkFamily::Evm => false,
            NetworkFamily::Solana => true,
        }
    }

    async fn payment_payload(
        &self,
        selected: PaymentRequirements,
    ) -> Result<PaymentPayload, X402PaymentsError> {
        let asset: SolanaAddress = selected.asset.clone().try_into().map_err(|e| {
            X402PaymentsError::SigningError(format!(
                "failed to convert asset to SolanaAddress: {e}"
            ))
        })?;
        let mint = self.fetch_mint(&asset)?;
        // create the ATA (if needed)
        let fee_payer = selected
            .extra
            .clone()
            .and_then(|k| k.get("feePayer").cloned())
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .and_then(|s| s.parse::<Pubkey>().ok())
            .ok_or(X402PaymentsError::SigningError(
                "failed to parse fee_payer".to_string(),
            ))?;

        // get the expected receiver's ATA
        // findAssociatedTokenPda
        let program_id = Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
            .map_err(|e| X402PaymentsError::SigningError(format!("{e}")))?;
        let asset_address: SolanaAddress = asset.clone();
        let asset_address: Pubkey = asset_address.into();
        let pay_to_address: SolanaAddress = selected
            .pay_to
            .clone()
            .try_into()
            .map_err(|e| X402PaymentsError::SigningError(format!("{e}")))?;
        let pay_to_address: Pubkey = pay_to_address.into();
        let (ata, _) = Pubkey::find_program_address(
            // findAssociatedTokenPda
            &[
                pay_to_address.as_ref(),
                mint.token_program().as_ref(),
                asset_address.as_ref(),
            ],
            &program_id,
        );
        let ata_account = self
            .rpc_client
            .get_account_with_commitment(&ata, self.rpc_client.commitment())
            .map_err(|e| X402PaymentsError::SigningError(format!("{e}")))?
            .value;
        let create_ata_instruction = if ata_account.is_some() {
            None
        } else {
            // getCreateAssociatedTokenInstruction
            let funding_address = &fee_payer;
            let wallet_address = &pay_to_address;
            let token_mint_address = &asset_address;
            let token_program_id = mint.token_program();
            let instruction = create_associated_token_account_idempotent(
                funding_address,
                wallet_address,
                token_mint_address,
                token_program_id,
            );
            Some(instruction)
        };

        // createTransferInstruction
        let client_address = self.keypair.pubkey();
        let (source_ata, _) = Pubkey::find_program_address(
            // findAssociatedTokenPda
            &[
                client_address.as_ref(),
                mint.token_program().as_ref(),
                asset_address.as_ref(),
            ],
            &program_id,
        );
        let destination_ata = ata;
        let amount: u64 = selected
            .max_amount_required
            .0
            .try_into()
            .map_err(|e| X402PaymentsError::SigningError(format!("{e}")))?;
        let transfer_instruction = match mint {
            Mint::Token {
                decimals,
                token_program,
            } => {
                spl_token::instruction::transfer_checked(
                    &token_program,
                    &source_ata,
                    &asset_address,
                    &destination_ata,
                    &client_address,
                    &[], // extra signer pubkeys if using multisig; else empty
                    amount,
                    decimals,
                )
                .map_err(|e| X402PaymentsError::SigningError(format!("{e}")))?
            }
            Mint::Token2022 {
                decimals,
                token_program,
            } => {
                spl_token_2022::instruction::transfer_checked(
                    &token_program,
                    &source_ata,
                    &asset_address,
                    &destination_ata,
                    &client_address,
                    &[], // extra signer pubkeys if using multisig; else empty
                    amount,
                    decimals,
                )
                .map_err(|e| X402PaymentsError::SigningError(format!("{e}")))?
            }
        };
        let transfer_instructions = if let Some(create_ata_instruction) = create_ata_instruction {
            vec![create_ata_instruction, transfer_instruction]
        } else {
            vec![transfer_instruction]
        };

        // createTransferTransactionMessage
        // 1. Build a message to simulate the transfer, 1 microlamport priority fee
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .map_err(|e| X402PaymentsError::SigningError(format!("{e:?}")))?;
        let fee = get_priority_fee_micro_lamports(
            self.rpc_client.as_ref(),
            &[fee_payer, destination_ata, source_ata],
        )?;
        let (msg_to_sim, instructions) =
            build_message_to_simulate(fee_payer, &transfer_instructions, fee, recent_blockhash)?;
        // 2) Estimate CU via simulation
        let estimated_cu = estimate_compute_units(self.rpc_client.as_ref(), &msg_to_sim)?;
        // prepend the CU limit instruction
        let cu_ix = ComputeBudgetInstruction::set_compute_unit_limit(estimated_cu);
        let msg = {
            let instructions_original = instructions;
            // Order: [CU limit] + [compute price] + transfer instructions
            let mut instructions = Vec::with_capacity(instructions_original.len() + 1);
            instructions.push(cu_ix);
            instructions.extend(instructions_original.clone());
            MessageV0::try_compile(&fee_payer, &instructions, &[], recent_blockhash)
                .map_err(|e| X402PaymentsError::SigningError(format!("{e:?}")))?
        };
        let tx = VersionedTransaction {
            signatures: vec![],
            message: VersionedMessage::V0(msg),
        };
        let tx = TransactionInt::new(tx);
        let signed = tx
            .sign(self.keypair.as_ref())
            .map_err(|e| X402PaymentsError::SigningError(format!("{e:?}")))?;
        let tx_b64 = signed
            .as_base64()
            .map_err(|e| X402PaymentsError::SigningError(format!("{e:?}")))?;

        let payment_payload = PaymentPayload {
            x402_version: X402Version::V1,
            scheme: selected.scheme,
            network: selected.network,
            payload: ExactPaymentPayload::Solana(ExactSolanaPayload {
                transaction: tx_b64,
            }),
        };
        Ok(payment_payload)
    }
}

#[derive(Debug)]
enum Mint {
    Token { decimals: u8, token_program: Pubkey },
    Token2022 { decimals: u8, token_program: Pubkey },
}

impl Mint {
    pub fn token_program(&self) -> &Pubkey {
        match self {
            Mint::Token { token_program, .. } => token_program,
            Mint::Token2022 { token_program, .. } => token_program,
        }
    }
}

//  Build the message we want to simulate (priority fee + transfer Ixs).
pub fn build_message_to_simulate(
    fee_payer: Pubkey,
    transfer_instructions: &[Instruction],
    priority_micro_lamports: u64,
    recent_blockhash: Hash,
) -> Result<(MessageV0, Vec<Instruction>), X402PaymentsError> {
    // Same as setTransactionMessageComputeUnitPrice(priority_micro_lamports)
    let set_price = ComputeBudgetInstruction::set_compute_unit_price(priority_micro_lamports);

    // Order: [compute price] + transfer Ixs
    let mut ixs = Vec::with_capacity(1 + transfer_instructions.len());
    ixs.push(set_price);
    ixs.extend(transfer_instructions.to_owned());
    // and maybe CU limit instruction at the end
    let with_cu_limit = {
        let mut ixs_mod = ixs.clone();
        update_or_append_set_compute_unit_limit(&mut ixs_mod, 1e5 as u32);
        ixs_mod
    };
    let message = MessageV0::try_compile(&fee_payer, &with_cu_limit, &[], recent_blockhash)
        .map_err(|e| X402PaymentsError::SigningError(format!("{e:?}")))?;
    Ok((message, ixs))
}

/// 2) Estimate compute units by simulating the unsigned/signed tx. (We sign with the fee payer so simulation accepts it.)
pub fn estimate_compute_units(
    rpc: &RpcClient,
    message: &MessageV0,
) -> Result<u32, X402PaymentsError> {
    // Can not use `VersionedTransaction::try_new` because it checks if transaction is fully signed. Not our case here.
    let message = VersionedMessage::V0(message.clone());
    let num_required_signatures = message.header().num_required_signatures;
    let tx = VersionedTransaction {
        // If we pass an empty vector here, the transaction simulation fails.
        signatures: vec![Signature::default(); num_required_signatures as usize],
        message,
    };

    let sim = rpc
        .simulate_transaction_with_config(
            &tx,
            RpcSimulateTransactionConfig {
                sig_verify: false,
                replace_recent_blockhash: true,
                ..RpcSimulateTransactionConfig::default()
            },
        )
        .map_err(|e| X402PaymentsError::SigningError(format!("{e:?}")))?;
    let units = sim
        .value
        .units_consumed
        .ok_or(X402PaymentsError::SigningError(
            "simulation returned no units_consumed".to_string(),
        ))?;
    Ok(units as u32)
}

pub fn get_priority_fee_micro_lamports(
    rpc: &RpcClient,
    writeable_accounts: &[Pubkey],
) -> Result<u64, X402PaymentsError> {
    let fee = rpc
        .get_recent_prioritization_fees(writeable_accounts)
        .map_err(|e| X402PaymentsError::SigningError(format!("{e:?}")))?
        .iter()
        .filter(|e| e.prioritization_fee > 0)
        .map(|e| e.prioritization_fee)
        .min_by(|a, b| a.cmp(b))
        .unwrap_or(1);

    Ok(fee)
}

/// Update the first set_compute_unit_limit ix if it exists, else append a new one.
pub fn update_or_append_set_compute_unit_limit(ixs: &mut Vec<Instruction>, units: u32) {
    // opcode 0x02 = SetComputeUnitLimit
    let target_program = solana_sdk::compute_budget::id();
    let new_ix = ComputeBudgetInstruction::set_compute_unit_limit(units);

    // find first ix targeting the ComputeBudget program with opcode=2
    if let Some(ix) = ixs
        .iter_mut()
        .find(|ix| ix.program_id == target_program && !ix.data.is_empty() && ix.data[0] == 2)
    {
        *ix = new_ix; // replace
    } else {
        ixs.push(new_ix); // append
    }
}
