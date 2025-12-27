use async_trait::async_trait;
use serde::Deserialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_keypair::Keypair;
use solana_message::v0::Message as MessageV0;
use solana_message::{Hash, VersionedMessage};
use solana_pubkey::Pubkey;
use solana_signature::Signature;
use solana_signer::Signer;
use solana_transaction::versioned::VersionedTransaction;
use spl_token::solana_program::program_pack::Pack;
use std::sync::Arc;
use alloy_primitives::U256;
use solana_transaction::Instruction;
use crate::chain::solana::Address;
use crate::chain::ChainId;
use crate::proto::client::{PaymentCandidate, PaymentCandidateSigner, X402Error, X402SchemeClient};
use crate::proto::v1::X402Version1;
use crate::proto::PaymentRequired;
use crate::scheme::v1_solana_exact::types::{
    ExactScheme, ExactSolanaPayload, PaymentPayload, PaymentRequirements,
};
use crate::scheme::v1_solana_exact::{TransactionInt, V1SolanaExact, ATA_PROGRAM_PUBKEY};
use crate::scheme::X402SchemeId;
use crate::util::Base64Bytes;

/// Client for creating Solana payment payloads for the v1 exact scheme.
///
/// This client handles the creation of SPL Token transfer transactions
/// that can be used to pay for x402-protected resources.
#[derive(Clone)]
pub struct V1SolanaExactClient {
    keypair: Arc<Keypair>,
    rpc_client: Arc<RpcClient>,
}

impl V1SolanaExactClient {
    /// Creates a new V1SolanaExactClient with the given keypair and RPC client.
    ///
    /// # Arguments
    /// * `keypair` - The Solana keypair used to sign transactions
    /// * `rpc_client` - The RPC client used to interact with the Solana network
    pub fn new(keypair: Keypair, rpc_client: RpcClient) -> Self {
        Self {
            keypair: Arc::new(keypair),
            rpc_client: Arc::new(rpc_client),
        }
    }
}

impl X402SchemeId for V1SolanaExactClient {
    fn x402_version(&self) -> u8 {
        V1SolanaExact.x402_version()
    }

    fn namespace(&self) -> &str {
        V1SolanaExact.namespace()
    }

    fn scheme(&self) -> &str {
        V1SolanaExact.scheme()
    }
}

impl X402SchemeClient for V1SolanaExactClient {
    fn accept(&self, payment_required: &PaymentRequired) -> Vec<PaymentCandidate> {
        let payment_required = match payment_required {
            PaymentRequired::V1(payment_required) => payment_required,
            PaymentRequired::V2(_) => {
                return vec![];
            }
        };
        payment_required
            .accepts
            .iter()
            .filter_map(|v| {
                let requirements = PaymentRequirements::deserialize(v).ok()?;
                // Check if this is a Solana network
                let chain_id = ChainId::from_network_name(&requirements.network)?;
                if chain_id.namespace != "solana" {
                    return None;
                }
                let candidate = PaymentCandidate {
                    chain_id,
                    asset: requirements.asset.to_string(),
                    amount: U256::from(requirements.max_amount_required.inner()),
                    scheme: self.scheme().to_string(),
                    x402_version: self.x402_version(),
                    pay_to: requirements.pay_to.to_string(),
                    signer: Box::new(PayloadSigner {
                        keypair: self.keypair.clone(),
                        rpc_client: self.rpc_client.clone(),
                        requirements,
                    }),
                };
                Some(candidate)
            })
            .collect::<Vec<_>>()
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

struct PayloadSigner {
    keypair: Arc<Keypair>,
    rpc_client: Arc<RpcClient>,
    requirements: PaymentRequirements,
}

impl PayloadSigner {
    async fn fetch_mint(&self, mint_address: &Address) -> Result<Mint, X402Error> {
        let mint_pubkey: Pubkey = mint_address.clone().into();
        let account = self.rpc_client.get_account(&mint_pubkey).await.map_err(|e| {
            X402Error::SigningError(format!("failed to fetch mint {mint_pubkey}: {e}"))
        })?;
        if account.owner == spl_token::id() {
            let mint = spl_token::state::Mint::unpack(&account.data).map_err(|e| {
                X402Error::SigningError(format!("failed to unpack mint {mint_pubkey}: {e}"))
            })?;
            Ok(Mint::Token {
                decimals: mint.decimals,
                token_program: spl_token::id(),
            })
        } else if account.owner == spl_token_2022::id() {
            let mint = spl_token_2022::state::Mint::unpack(&account.data).map_err(|e| {
                X402Error::SigningError(format!("failed to unpack mint {mint_pubkey}: {e}",))
            })?;
            Ok(Mint::Token2022 {
                decimals: mint.decimals,
                token_program: spl_token_2022::id(),
            })
        } else {
            Err(X402Error::SigningError(format!(
                "failed to unpack mint {mint_pubkey}: unknown owner"
            )))
        }
    }
}

#[async_trait]
impl PaymentCandidateSigner for PayloadSigner {
    async fn sign_payment(&self) -> Result<String, X402Error> {
        let asset: Address = self.requirements.asset.clone();
        let mint = self.fetch_mint(&asset).await?;

        // Get the fee payer from the extra field
        let fee_payer = self
            .requirements
            .extra
            .as_ref()
            .map(|extra| extra.fee_payer.clone())
            .ok_or(X402Error::SigningError(
                "missing fee_payer in extra".to_string(),
            ))?;
        let fee_payer_pubkey: Pubkey = fee_payer.into();

        // Get the expected receiver's ATA
        let asset_pubkey: Pubkey = asset.clone().into();
        let pay_to_pubkey: Pubkey = self.requirements.pay_to.clone().into();

        let (ata, _) = Pubkey::find_program_address(
            &[
                pay_to_pubkey.as_ref(),
                mint.token_program().as_ref(),
                asset_pubkey.as_ref(),
            ],
            &ATA_PROGRAM_PUBKEY,
        );

        // Create transfer instruction
        let client_pubkey = self.keypair.pubkey();
        let (source_ata, _) = Pubkey::find_program_address(
            &[
                client_pubkey.as_ref(),
                mint.token_program().as_ref(),
                asset_pubkey.as_ref(),
            ],
            &ATA_PROGRAM_PUBKEY,
        );
        let destination_ata = ata;
        let amount: u64 = self.requirements.max_amount_required.inner();

        let transfer_instruction = match mint {
            Mint::Token {
                decimals,
                token_program,
            } => spl_token::instruction::transfer_checked(
                &token_program,
                &source_ata,
                &asset_pubkey,
                &destination_ata,
                &client_pubkey,
                &[],
                amount,
                decimals,
            )
            .map_err(|e| X402Error::SigningError(format!("{e}")))?,
            Mint::Token2022 {
                decimals,
                token_program,
            } => spl_token_2022::instruction::transfer_checked(
                &token_program,
                &source_ata,
                &asset_pubkey,
                &destination_ata,
                &client_pubkey,
                &[],
                amount,
                decimals,
            )
            .map_err(|e| X402Error::SigningError(format!("{e}")))?,
        };

        let transfer_instructions = vec![transfer_instruction];

        // Build the transaction message
        let recent_blockhash = self
            .rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| X402Error::SigningError(format!("{e:?}")))?;

        let fee = get_priority_fee_micro_lamports(
            self.rpc_client.as_ref(),
            &[fee_payer_pubkey, destination_ata, source_ata],
        )
        .await?;

        let (msg_to_sim, instructions) = build_message_to_simulate(
            fee_payer_pubkey,
            &transfer_instructions,
            fee,
            recent_blockhash,
        )?;

        // Estimate compute units via simulation
        let estimated_cu = estimate_compute_units(self.rpc_client.as_ref(), &msg_to_sim).await?;

        // Build final message with CU limit
        let cu_ix = ComputeBudgetInstruction::set_compute_unit_limit(estimated_cu);
        let msg = {
            let mut final_instructions = Vec::with_capacity(instructions.len() + 1);
            final_instructions.push(cu_ix);
            final_instructions.extend(instructions);
            MessageV0::try_compile(&fee_payer_pubkey, &final_instructions, &[], recent_blockhash)
                .map_err(|e| X402Error::SigningError(format!("{e:?}")))?
        };

        let tx = VersionedTransaction {
            signatures: vec![],
            message: VersionedMessage::V0(msg),
        };

        let tx = TransactionInt::new(tx);
        let signed = tx
            .sign_with_keypair(self.keypair.as_ref())
            .map_err(|e| X402Error::SigningError(format!("{e:?}")))?;
        let tx_b64 = signed
            .as_base64()
            .map_err(|e| X402Error::SigningError(format!("{e:?}")))?;

        // Build the payment payload
        let payload = PaymentPayload {
            x402_version: X402Version1,
            scheme: ExactScheme,
            network: self.requirements.network.clone(),
            payload: ExactSolanaPayload { transaction: tx_b64 },
        };
        let json = serde_json::to_vec(&payload)?;
        let b64 = Base64Bytes::encode(&json);

        Ok(b64.to_string())
    }
}

/// Build the message we want to simulate (priority fee + transfer Ixs).
pub fn build_message_to_simulate(
    fee_payer: Pubkey,
    transfer_instructions: &[Instruction],
    priority_micro_lamports: u64,
    recent_blockhash: Hash,
) -> Result<(MessageV0, Vec<Instruction>), X402Error> {
    let set_price = ComputeBudgetInstruction::set_compute_unit_price(priority_micro_lamports);

    let mut ixs = Vec::with_capacity(1 + transfer_instructions.len());
    ixs.push(set_price);
    ixs.extend(transfer_instructions.to_owned());

    let with_cu_limit = {
        let mut ixs_mod = ixs.clone();
        update_or_append_set_compute_unit_limit(&mut ixs_mod, 1e5 as u32);
        ixs_mod
    };
    let message = MessageV0::try_compile(&fee_payer, &with_cu_limit, &[], recent_blockhash)
        .map_err(|e| X402Error::SigningError(format!("{e:?}")))?;
    Ok((message, ixs))
}

/// Estimate compute units by simulating the unsigned/signed tx.
pub async fn estimate_compute_units(rpc: &RpcClient, message: &MessageV0) -> Result<u32, X402Error> {
    let message = VersionedMessage::V0(message.clone());
    let num_required_signatures = message.header().num_required_signatures;
    let tx = VersionedTransaction {
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
        .await
        .map_err(|e| X402Error::SigningError(format!("{e:?}")))?;
    let units = sim.value.units_consumed.ok_or(X402Error::SigningError(
        "simulation returned no units_consumed".to_string(),
    ))?;
    Ok(units as u32)
}

/// Get the priority fee in micro-lamports.
pub async fn get_priority_fee_micro_lamports(
    rpc: &RpcClient,
    writeable_accounts: &[Pubkey],
) -> Result<u64, X402Error> {
    let fee = rpc
        .get_recent_prioritization_fees(writeable_accounts)
        .await
        .map_err(|e| X402Error::SigningError(format!("{e:?}")))?
        .iter()
        .filter(|e| e.prioritization_fee > 0)
        .map(|e| e.prioritization_fee)
        .min_by(|a, b| a.cmp(b))
        .unwrap_or(1);

    Ok(fee)
}

/// Update the first set_compute_unit_limit ix if it exists, else append a new one.
pub fn update_or_append_set_compute_unit_limit(
    ixs: &mut Vec<solana_transaction::Instruction>,
    units: u32,
) {
    let target_program = solana_compute_budget_interface::ID;
    let new_ix = ComputeBudgetInstruction::set_compute_unit_limit(units);

    if let Some(ix) = ixs
        .iter_mut()
        .find(|ix| ix.program_id == target_program && !ix.data.is_empty() && ix.data[0] == 2)
    {
        *ix = new_ix;
    } else {
        ixs.push(new_ix);
    }
}
