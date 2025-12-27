use alloy_primitives::U256;
use async_trait::async_trait;
use serde::Deserialize;
use solana_pubkey::Pubkey;
use solana_signer::Signer;

use crate::proto::PaymentRequired;
use crate::proto::client::{PaymentCandidate, PaymentCandidateSigner, X402Error, X402SchemeClient};
use crate::proto::v2;
use crate::proto::v2::ResourceInfo;
use crate::proto::v2::X402Version2;
use crate::scheme::X402SchemeId;
use crate::scheme::v1_solana_exact::client::{RpcClientLike, build_signed_transfer_transaction};
use crate::scheme::v1_solana_exact::types::ExactSolanaPayload;
use crate::scheme::v2_solana_exact::V2SolanaExact;
use crate::scheme::v2_solana_exact::types::{PaymentPayload, PaymentRequirements};
use crate::util::Base64Bytes;

/// Client for creating Solana payment payloads for the v2 exact scheme.
#[derive(Clone)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct V2SolanaExactClient<S, R> {
    signer: S,
    rpc_client: R,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl<S, R> V2SolanaExactClient<S, R> {
    pub fn new(signer: S, rpc_client: R) -> Self {
        Self { signer, rpc_client }
    }
}

impl<S, R> X402SchemeId for V2SolanaExactClient<S, R> {
    fn x402_version(&self) -> u8 {
        V2SolanaExact.x402_version()
    }

    fn namespace(&self) -> &str {
        V2SolanaExact.namespace()
    }

    fn scheme(&self) -> &str {
        V2SolanaExact.scheme()
    }
}

impl<S, R> X402SchemeClient for V2SolanaExactClient<S, R>
where
    S: Signer + Send + Sync + Clone + 'static,
    R: RpcClientLike + Send + Sync + Clone + 'static,
{
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
                        signer: self.signer.clone(),
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

/// V2 PayloadSigner that uses shared transaction building logic.
#[allow(dead_code)] // Public for consumption by downstream crates.
struct PayloadSigner<S, R> {
    signer: S,
    rpc_client: R,
    requirements: PaymentRequirements,
    resource: ResourceInfo,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
#[async_trait]
impl<S: Signer + Sync, R: RpcClientLike + Sync> PaymentCandidateSigner for PayloadSigner<S, R> {
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
            &self.signer,
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
