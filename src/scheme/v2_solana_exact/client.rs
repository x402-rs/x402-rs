use alloy_primitives::U256;
use async_trait::async_trait;
use serde::Deserialize;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_keypair::Keypair;
use solana_pubkey::Pubkey;
use std::sync::Arc;

use crate::proto::PaymentRequired;
use crate::proto::client::{PaymentCandidate, PaymentCandidateSigner, X402Error, X402SchemeClient};
use crate::proto::v2;
use crate::proto::v2::ResourceInfo;
use crate::proto::v2::X402Version2;
use crate::scheme::X402SchemeId;
use crate::scheme::v1_solana_exact::client::build_signed_transfer_transaction;
use crate::scheme::v1_solana_exact::types::ExactSolanaPayload;
use crate::scheme::v2_solana_exact::V2SolanaExact;
use crate::scheme::v2_solana_exact::types::{PaymentPayload, PaymentRequirements};
use crate::util::Base64Bytes;

/// Client for creating Solana payment payloads for the v2 exact scheme.
#[derive(Clone)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct V2SolanaExactClient {
    keypair: Arc<Keypair>,
    rpc_client: Arc<RpcClient>,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl V2SolanaExactClient {
    pub fn new(keypair: Keypair, rpc_client: RpcClient) -> Self {
        Self {
            keypair: Arc::new(keypair),
            rpc_client: Arc::new(rpc_client),
        }
    }
}

impl X402SchemeId for V2SolanaExactClient {
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
                let requirements: PaymentRequirements =
                    v2::PaymentRequirements::deserialize(v).ok()?;
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
                        keypair: Arc::clone(&self.keypair),
                        rpc_client: Arc::clone(&self.rpc_client),
                        requirements,
                        resource: payment_required.resource.clone(),
                    }),
                };
                Some(candidate)
            })
            .collect::<Vec<_>>()
    }
}

/// V2 PayloadSigner that uses shared transaction building logic.
#[allow(dead_code)] // Public for consumption by downstream crates.
struct PayloadSigner {
    keypair: Arc<Keypair>,
    rpc_client: Arc<RpcClient>,
    requirements: PaymentRequirements,
    resource: ResourceInfo,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
#[async_trait]
impl PaymentCandidateSigner for PayloadSigner {
    async fn sign_payment(&self) -> Result<String, X402Error> {
        let fee_payer = self
            .requirements
            .extra
            .as_ref()
            .map(|extra| extra.fee_payer.clone())
            .ok_or(X402Error::SigningError(
                "missing fee_payer in extra".to_string(),
            ))?;
        let fee_payer_pubkey: Pubkey = fee_payer.into();

        let amount = self.requirements.amount.inner();
        let tx_b64 = build_signed_transfer_transaction(
            &self.keypair,
            &self.rpc_client,
            &fee_payer_pubkey,
            &self.requirements.pay_to,
            &self.requirements.asset,
            amount,
        )
        .await?;

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
