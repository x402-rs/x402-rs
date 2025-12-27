use alloy_primitives::U256;
use async_trait::async_trait;
use serde::Deserialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use std::sync::Arc;
use solana_message::VersionedMessage;
use solana_transaction::versioned::VersionedTransaction;

use crate::proto::v2::ResourceInfo;
use crate::proto::v2::X402Version2;
use crate::proto::client::{PaymentCandidate, PaymentCandidateSigner, X402Error, X402SchemeClient};
use crate::proto::PaymentRequired;
use crate::proto::v2;
use crate::scheme::X402SchemeId;
use crate::scheme::v1_solana_exact::client::{
    build_message_to_simulate, estimate_compute_units, fetch_mint,
    get_priority_fee_micro_lamports, Mint,
};
use crate::scheme::v2_solana_exact::types::{PaymentPayload, PaymentRequirements};
use crate::scheme::v2_solana_exact::V2SolanaExact;
use crate::scheme::v1_solana_exact::{TransactionInt, ATA_PROGRAM_PUBKEY};
use crate::scheme::v1_solana_exact::types::ExactSolanaPayload;
use crate::util::Base64Bytes;

/// Client for creating Solana payment payloads for the v2 exact scheme.
///
/// This client handles the creation of SPL Token transfer transactions
/// that can be used to pay for x402-protected resources.
#[derive(Clone)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct V2SolanaExactClient {
    keypair: Arc<Keypair>,
    rpc_client: Arc<RpcClient>,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl V2SolanaExactClient {
    /// Creates a new V2SolanaExactClient with the given keypair and RPC client.
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

impl X402SchemeId for V2SolanaExactClient {
    fn x402_version(&self) -> u8 {
        2
    }

    fn namespace(&self) -> &str {
        V2SolanaExact.namespace()
    }

    fn scheme(&self) -> &str {
        V2SolanaExact.scheme()
    }
}

impl X402SchemeClient for V2SolanaExactClient {
    fn accept(&self, payment_required: &PaymentRequired) -> Vec<PaymentCandidate> {
        let payment_required = match payment_required {
            PaymentRequired::V2(payment_required) => payment_required,
            PaymentRequired::V1(_) => {
                return vec![];
            }
        };
        payment_required
            .accepts
            .iter()
            .filter_map(|v| {
                let requirements: PaymentRequirements = v2::PaymentRequirements::deserialize(v).ok()?;
                // Check if this is a Solana network
                let chain_id = requirements.network.clone();
                if chain_id.namespace != "solana" {
                    return None;
                }
                let candidate = PaymentCandidate {
                    chain_id,
                    asset: requirements.asset.to_string(),
                    amount: U256::from(requirements.amount.inner()),
                    scheme: self.scheme().to_string(),
                    x402_version: self.x402_version(),
                    pay_to: requirements.pay_to.to_string(),
                    signer: Box::new(PayloadSigner {
                        keypair: self.keypair.clone(),
                        rpc_client: self.rpc_client.clone(),
                        requirements,
                        resource: payment_required.resource.clone(),
                    }),
                };
                Some(candidate)
            })
            .collect::<Vec<_>>()
    }
}

/// V2 PayloadSigner that uses shared utilities from v1
#[allow(dead_code)] // Public for consumption by downstream crates.
struct PayloadSigner {
    keypair: Arc<Keypair>,
    rpc_client: Arc<RpcClient>,
    requirements: PaymentRequirements,
    resource: ResourceInfo,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl PayloadSigner {
    async fn build_transaction(&self) -> Result<String, X402Error> {
        let asset = &self.requirements.asset;
        let mint = fetch_mint(asset, &self.rpc_client).await?;

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
        let pay_to = &self.requirements.pay_to;

        let (ata, _) = Pubkey::find_program_address(
            &[
                pay_to.as_ref(),
                mint.token_program().as_ref(),
                asset.as_ref(),
            ],
            &ATA_PROGRAM_PUBKEY,
        );

        // Create transfer instruction
        let client_pubkey = self.keypair.pubkey();
        let (source_ata, _) = Pubkey::find_program_address(
            &[
                client_pubkey.as_ref(),
                mint.token_program().as_ref(),
                asset.as_ref(),
            ],
            &ATA_PROGRAM_PUBKEY,
        );
        let destination_ata = ata;
        let amount: u64 = self.requirements.amount.inner();

        let transfer_instruction = match mint {
            Mint::Token {
                decimals,
                token_program,
            } => spl_token::instruction::transfer_checked(
                &token_program,
                &source_ata,
                asset.pubkey(),
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
                asset.pubkey(),
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
            solana_message::v0::Message::try_compile(
                &fee_payer_pubkey,
                &final_instructions,
                &[],
                recent_blockhash,
            )
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

        Ok(tx_b64)
    }
}

#[allow(dead_code)] // Public for consumption by downstream crates.
#[async_trait]
impl PaymentCandidateSigner for PayloadSigner {
    async fn sign_payment(&self) -> Result<String, X402Error> {
        let tx_b64 = self.build_transaction().await?;

        // Build the v2 payment payload
        let payload = PaymentPayload {
            x402_version: X402Version2,
            accepted: self.requirements.clone(),
            resource: self.resource.clone(),
            payload: ExactSolanaPayload {
                transaction: tx_b64,
            },
        };
        let json = serde_json::to_vec(&payload)?;
        let b64 = Base64Bytes::encode(&json);

        Ok(b64.to_string())
    }
}
