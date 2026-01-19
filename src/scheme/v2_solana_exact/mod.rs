//! V2 Solana "exact" payment scheme implementation.
//!
//! This module implements the "exact" payment scheme for Solana using
//! the V2 x402 protocol. It builds on the V1 implementation but uses
//! CAIP-2 chain identifiers instead of network names.
//!
//! # Differences from V1
//!
//! - Uses CAIP-2 chain IDs (e.g., `solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp`) instead of network names
//! - Payment requirements are embedded in the payload for verification
//! - Cleaner separation between accepted requirements and authorization
//!
//! # Features
//!
//! - SPL Token and Token-2022 program support
//! - Compute budget instruction validation
//! - Transaction simulation before settlement
//! - Fee payer safety checks
//! - Configurable instruction allowlists/blocklists
//!
//! # Usage
//!
//! ```ignore
//! use x402::scheme::v2_solana_exact::V2SolanaExact;
//! use x402::networks::{KnownNetworkSolana, USDC};
//!
//! // Create a price tag for 1 USDC on Solana mainnet
//! let usdc = USDC::solana_mainnet();
//! let price = V2SolanaExact::price_tag(
//!     "recipient_pubkey...",  // pay_to address
//!     usdc.amount(1_000_000),  // 1 USDC
//! );
//! ```

pub mod client;
pub mod types;

use std::collections::HashMap;
use std::sync::Arc;

use crate::chain::ChainProvider;
use crate::chain::solana::{Address, SolanaChainProvider, SolanaTokenDeployment};
use crate::chain::{ChainId, ChainProviderOps, DeployedTokenAmount};
use crate::proto;
use crate::proto::v2;
use crate::scheme::v1_solana_exact::types::ExactScheme;
use crate::scheme::v1_solana_exact::types::SupportedPaymentKindExtra;
use crate::scheme::v1_solana_exact::{
    TransferRequirement, VerifyTransferResult, settle_transaction, verify_transaction,
};
use crate::scheme::{
    X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError, X402SchemeId,
};
use types::V2SolanaExactFacilitatorConfig;

pub struct V2SolanaExact;

impl V2SolanaExact {
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn price_tag<A: Into<Address>>(
        pay_to: A,
        asset: DeployedTokenAmount<u64, SolanaTokenDeployment>,
    ) -> v2::PriceTag {
        let chain_id: ChainId = asset.token.chain_reference.into();
        let requirements = v2::PaymentRequirements {
            scheme: ExactScheme.to_string(),
            pay_to: pay_to.into().to_string(),
            asset: asset.token.address.to_string(),
            network: chain_id,
            amount: asset.amount.to_string(),
            max_timeout_seconds: 300,
            extra: None,
        };
        v2::PriceTag {
            requirements,
            enricher: Some(Arc::new(solana_fee_payer_enricher_v2)),
        }
    }
}

impl X402SchemeId for V2SolanaExact {
    fn namespace(&self) -> &str {
        "solana"
    }

    fn scheme(&self) -> &str {
        ExactScheme.as_ref()
    }
}

impl X402SchemeFacilitatorBuilder<&ChainProvider> for V2SolanaExact {
    fn build(
        &self,
        provider: &ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let solana_provider = if let ChainProvider::Solana(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V2SolanaExact::build: provider must be a SolanaChainProvider".into());
        };
        self.build(solana_provider, config)
    }
}

impl X402SchemeFacilitatorBuilder<Arc<SolanaChainProvider>> for V2SolanaExact {
    fn build(
        &self,
        provider: Arc<SolanaChainProvider>,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let config = config
            .map(serde_json::from_value::<V2SolanaExactFacilitatorConfig>)
            .transpose()?
            .unwrap_or_default();

        Ok(Box::new(V2SolanaExactFacilitator::new(provider, config)))
    }
}

pub struct V2SolanaExactFacilitator {
    provider: Arc<SolanaChainProvider>,
    config: V2SolanaExactFacilitatorConfig,
}

impl V2SolanaExactFacilitator {
    pub fn new(provider: Arc<SolanaChainProvider>, config: V2SolanaExactFacilitatorConfig) -> Self {
        Self { provider, config }
    }
}

#[async_trait::async_trait]
impl X402SchemeFacilitator for V2SolanaExactFacilitator {
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

pub async fn verify_transfer(
    provider: &SolanaChainProvider,
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

/// Enricher function for V2 Solana price tags - adds fee_payer to extra field
pub fn solana_fee_payer_enricher_v2(
    price_tag: &mut v2::PriceTag,
    capabilities: &proto::SupportedResponse,
) {
    if price_tag.requirements.extra.is_some() {
        return;
    }

    // Find the matching kind and deserialize the whole extra into SupportedPaymentKindExtra
    let extra = capabilities
        .kinds
        .iter()
        .find(|kind| {
            v2::X402Version2 == kind.x402_version
                && kind.scheme == ExactScheme.to_string()
                && kind.network == price_tag.requirements.network.to_string()
        })
        .and_then(|kind| kind.extra.as_ref())
        .and_then(|extra| serde_json::from_value::<SupportedPaymentKindExtra>(extra.clone()).ok());

    // Serialize the whole extra back to Value
    if let Some(extra) = extra {
        price_tag.requirements.extra = serde_json::to_value(&extra).ok();
    }
}
