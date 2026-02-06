//! Client-side payment signing for the V2 Solana "exact" scheme.
//!
//! This module provides [`V2SolanaExactClient`] for building and signing
//! SPL Token transfer transactions on Solana using the V2 protocol.
//!
//! # Usage
//!
//! ```rust
//! use x402_chain_solana::v2_solana_exact::client::V2SolanaExactClient;
//! use solana_client::nonblocking::rpc_client::RpcClient;
//! use solana_keypair::Keypair;
//!
//! let keypair = Keypair::new();
//! let rpc = RpcClient::new("https://api.mainnet-beta.solana.com".to_string());
//! let client = V2SolanaExactClient::new(keypair, rpc);
//! ```

use alloy_primitives::U256;
use async_trait::async_trait;
use solana_pubkey::Pubkey;
use solana_signer::Signer;
use x402_types::proto::v2::ResourceInfo;
use x402_types::proto::v2::X402Version2;
use x402_types::proto::{OriginalJson, PaymentRequired};
use x402_types::scheme::X402SchemeId;
use x402_types::scheme::client::{
    PaymentCandidate, PaymentCandidateSigner, X402Error, X402SchemeClient,
};
use x402_types::util::Base64Bytes;

use crate::chain::rpc::RpcClientLike;
use crate::v1_solana_exact::client::build_signed_transfer_transaction;
use crate::v1_solana_exact::types::ExactSolanaPayload;
use crate::v2_solana_exact::V2SolanaExact;
use crate::v2_solana_exact::types::{PaymentPayload, PaymentRequirements};

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
            .filter_map(|original_requirements_json| {
                let requirements =
                    PaymentRequirements::try_from(original_requirements_json).ok()?;
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
                        resource: payment_required.resource.clone(),
                        requirements,
                        requirements_json: original_requirements_json.clone(),
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
    resource: ResourceInfo,
    requirements: PaymentRequirements,
    requirements_json: OriginalJson,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
#[async_trait]
impl<S: Signer + Sync, R: RpcClientLike + Sync> PaymentCandidateSigner for PayloadSigner<S, R> {
    async fn sign_payment(&self) -> Result<String, X402Error> {
        let fee_payer = self.requirements.extra.fee_payer.clone();
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
            accepted: self.requirements_json.clone(),
            resource: Some(self.resource.clone()),
            payload: ExactSolanaPayload {
                transaction: tx_b64,
            },
        };
        let json = serde_json::to_vec(&payload)?;
        let b64 = Base64Bytes::encode(&json);

        Ok(b64.to_string())
    }
}
