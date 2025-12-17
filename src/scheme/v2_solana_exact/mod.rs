mod types;

use crate::chain::solana::SolanaChainProvider;
use crate::chain::{ChainProvider, ChainProviderOps};
use crate::facilitator_local::FacilitatorLocalError;
use crate::proto;
use crate::scheme::v1_eip155_exact::EXACT_SCHEME;
use crate::scheme::v1_solana_exact::types::SupportedPaymentKindExtra;
use crate::scheme::v1_solana_exact::{
    TransferRequirement, VerifyTransferResult, settle_transaction, verify_transaction,
};
use crate::scheme::{SchemeSlug, X402SchemeBlueprint, X402SchemeHandler};
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

// FIXME How to create a scheme
// 1. start with declaring a possibly empty struct for your scheme
pub struct V2SolanaExact;

// 2. Define impl X402SchemeBlueprint
// 3. There: (a) - prepare correct slug
// (b) Make the handler ib (build) fn
// 4. Implement X402SchemeHandler for the handler - do not forget to mark the trait as async_trait
impl X402SchemeBlueprint for V2SolanaExact {
    fn slug(&self) -> SchemeSlug {
        SchemeSlug::new(2, "solana", EXACT_SCHEME.to_string())
    }

    fn build(
        &self,
        provider: ChainProvider,
        _config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeHandler>, Box<dyn Error>> {
        let provider = if let ChainProvider::Solana(provider) = provider {
            provider
        } else {
            return Err("V1SolanaExact::build: provider must be a SolanaChainProvider".into());
        };
        Ok(Box::new(V2SolanaExactHandler { provider }))
    }
}

pub struct V2SolanaExactHandler {
    provider: Arc<SolanaChainProvider>,
}

#[async_trait::async_trait]
impl X402SchemeHandler for V2SolanaExactHandler {
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, FacilitatorLocalError> {
        let request = types::VerifyRequest::from_proto(request.clone()).ok_or(
            FacilitatorLocalError::DecodingError("Can not decode payload".to_string()),
        )?;
        let verification = verify_transfer(&self.provider, &request).await?;
        Ok(proto::v2::VerifyResponse::valid(verification.payer.to_string()).into())
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, FacilitatorLocalError> {
        let request = types::SettleRequest::from_proto(request.clone()).ok_or(
            FacilitatorLocalError::DecodingError("Can not decode payload".to_string()),
        )?;
        let verification = verify_transfer(&self.provider, &request).await?;
        let payer = verification.payer.to_string();
        let tx_sig = settle_transaction(&self.provider, verification).await?;
        Ok(proto::v2::SettleResponse::Success {
            payer,
            transaction: tx_sig.to_string(),
            network: self.provider.chain_id().to_string(),
        }
        .into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, FacilitatorLocalError> {
        let chain_id = self.provider.chain_id();
        let kinds: Vec<proto::SupportedPaymentKind> = {
            let fee_payer = self.provider.fee_payer();
            let extra =
                Some(serde_json::to_value(SupportedPaymentKindExtra { fee_payer }).unwrap());
            vec![proto::SupportedPaymentKind {
                x402_version: proto::v2::X402Version2.into(),
                scheme: EXACT_SCHEME.to_string(),
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

pub async fn verify_transfer(
    provider: &SolanaChainProvider,
    request: &types::VerifyRequest,
) -> Result<VerifyTransferResult, FacilitatorLocalError> {
    let payload = &request.payment_payload;
    let requirements = &request.payment_requirements;

    let accepted = &payload.accepted;
    if accepted != requirements {
        return Err(FacilitatorLocalError::DecodingError(
            "Accepted requirements do not match payload requirements".to_string(),
        ));
    }

    let chain_id = provider.chain_id();
    let payload_chain_id = &accepted.network;
    if payload_chain_id != &chain_id {
        return Err(FacilitatorLocalError::NetworkMismatch(
            None,
            chain_id.to_string(),
            payload_chain_id.to_string(),
        ));
    }
    let transaction_b64_string = payload.payload.transaction.clone();
    let transfer_requirement = TransferRequirement {
        pay_to: &requirements.pay_to,
        asset: &requirements.asset,
        amount: requirements.amount.inner(),
    };
    verify_transaction(provider, transaction_b64_string, &transfer_requirement).await
}
