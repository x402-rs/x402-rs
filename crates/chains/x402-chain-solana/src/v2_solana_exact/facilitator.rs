#![cfg(feature = "facilitator")]

use std::collections::HashMap;
use x402_types::chain::ChainProviderOps;
use x402_types::proto;
use x402_types::proto::v2;
use x402_types::scheme::{
    X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError,
};

use crate::V2SolanaExact;
use crate::chain::provider::SolanaChainProviderLike;
use crate::v1_solana_exact::facilitator::V1SolanaExactFacilitatorConfig;
use crate::v1_solana_exact::facilitator::{
    TransferRequirement, VerifyTransferResult, settle_transaction, verify_transaction,
};
use crate::v1_solana_exact::types::SupportedPaymentKindExtra;
use crate::v2_solana_exact::types;

/// Configuration for V2 Solana Exact facilitator - reuses V1 config
pub type V2SolanaExactFacilitatorConfig = V1SolanaExactFacilitatorConfig;

impl<P> X402SchemeFacilitatorBuilder<P> for V2SolanaExact
where
    P: SolanaChainProviderLike + ChainProviderOps + Send + Sync + 'static,
{
    fn build(
        &self,
        provider: P,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let config = config
            .map(serde_json::from_value::<V2SolanaExactFacilitatorConfig>)
            .transpose()?
            .unwrap_or_default();

        Ok(Box::new(V2SolanaExactFacilitator::new(provider, config)))
    }
}

pub struct V2SolanaExactFacilitator<P> {
    provider: P,
    config: V2SolanaExactFacilitatorConfig,
}

impl<P> V2SolanaExactFacilitator<P> {
    pub fn new(provider: P, config: V2SolanaExactFacilitatorConfig) -> Self {
        Self { provider, config }
    }
}

#[async_trait::async_trait]
impl<P> X402SchemeFacilitator for V2SolanaExactFacilitator<P>
where
    P: SolanaChainProviderLike + ChainProviderOps + Send + Sync,
{
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError> {
        let request = types::VerifyRequest::from_proto(request.clone())?;
        let verification = verify_transfer(&self.provider, &request, &self.config).await?;
        Ok(v2::VerifyResponse::valid(verification.payer.to_string()).into())
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, X402SchemeFacilitatorError> {
        let request = types::SettleRequest::from_proto(request.clone())?;
        let verification = verify_transfer(&self.provider, &request, &self.config).await?;
        let payer = verification.payer.to_string();
        let tx_sig = settle_transaction(&self.provider, verification).await?;
        Ok(v2::SettleResponse::Success {
            payer,
            transaction: tx_sig.to_string(),
            network: self.provider.chain_id().to_string(),
        }
        .into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError> {
        let chain_id = self.provider.chain_id();
        let kinds: Vec<proto::SupportedPaymentKind> = {
            let fee_payer = self.provider.fee_payer();
            let extra =
                Some(serde_json::to_value(SupportedPaymentKindExtra { fee_payer }).unwrap());
            vec![proto::SupportedPaymentKind {
                x402_version: proto::v2::X402Version2.into(),
                scheme: types::ExactScheme.to_string(),
                network: chain_id.to_string(),
                extra,
            }]
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

pub async fn verify_transfer<P: SolanaChainProviderLike + ChainProviderOps>(
    provider: &P,
    request: &types::VerifyRequest,
    config: &V2SolanaExactFacilitatorConfig,
) -> Result<VerifyTransferResult, proto::PaymentVerificationError> {
    let payload = &request.payment_payload;
    let requirements = &request.payment_requirements;

    let accepted = &payload.accepted;
    if accepted != requirements {
        return Err(proto::PaymentVerificationError::AcceptedRequirementsMismatch);
    }

    let chain_id = provider.chain_id();
    let payload_chain_id = &accepted.network;
    if payload_chain_id != &chain_id {
        return Err(proto::PaymentVerificationError::UnsupportedChain);
    }
    let transaction_b64_string = payload.payload.transaction.clone();
    let transfer_requirement = TransferRequirement {
        pay_to: &requirements.pay_to,
        asset: &requirements.asset,
        amount: requirements.amount.inner(),
    };
    verify_transaction(
        provider,
        transaction_b64_string,
        &transfer_requirement,
        config,
    )
    .await
}
