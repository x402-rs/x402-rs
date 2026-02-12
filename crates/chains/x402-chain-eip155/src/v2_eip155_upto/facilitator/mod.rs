//! Facilitator-side payment verification and settlement for V2 EIP-155 upto scheme.
//!
//! This module implements the facilitator logic for V2 protocol "upto" payments on EVM chains.
//! The upto scheme allows clients to authorize a maximum amount, with the actual settled amount
//! determined by the server at settlement time based on resource consumption.

pub mod permit2;

use alloy_provider::Provider;
use std::collections::HashMap;
use x402_types::chain::ChainProviderOps;
use x402_types::proto;
use x402_types::proto::v2;
use x402_types::scheme::{
    X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError,
};

use crate::V2Eip155Upto;
use crate::chain::Eip155MetaTransactionProvider;
use crate::v1_eip155_exact::facilitator::Eip155ExactError;
use crate::v2_eip155_upto::types;

impl<P> X402SchemeFacilitatorBuilder<P> for V2Eip155Upto
where
    P: Eip155MetaTransactionProvider + ChainProviderOps + Send + Sync + 'static,
    Eip155ExactError: From<P::Error>,
{
    fn build(
        &self,
        provider: P,
        _config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        Ok(Box::new(V2Eip155UptoFacilitator::new(provider)))
    }
}

/// Facilitator for V2 EIP-155 upto scheme payments.
///
/// This struct implements the [`X402SchemeFacilitator`] trait to provide payment
/// verification and settlement services for Permit2-based "upto" payments on EVM chains
/// using the V2 protocol.
///
/// # Key Differences from Exact Scheme
///
/// - The client authorizes a **maximum** amount
/// - The server settles for the **actual** amount used (can be less than max)
/// - Only Permit2 is supported (EIP-3009 requires exact amounts at signing time)
/// - Zero settlements are allowed (no on-chain transaction needed)
///
/// # Type Parameters
///
/// - `P`: The provider type, which must implement [`Eip155MetaTransactionProvider`]
///   and [`ChainProviderOps`]
pub struct V2Eip155UptoFacilitator<P> {
    provider: P,
}

impl<P> V2Eip155UptoFacilitator<P> {
    /// Creates a new V2 EIP-155 upto scheme facilitator with the given provider.
    pub fn new(provider: P) -> Self {
        Self { provider }
    }
}

#[async_trait::async_trait]
impl<P> X402SchemeFacilitator for V2Eip155UptoFacilitator<P>
where
    P: Eip155MetaTransactionProvider + ChainProviderOps + Send + Sync,
    P::Inner: Provider,
    Eip155ExactError: From<P::Error>,
{
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError> {
        let verify_request = types::VerifyRequest::try_from(request)?;
        let verify_response = permit2::verify_permit2_payment(
            &self.provider,
            &verify_request.payment_payload,
            &verify_request.payment_requirements,
        )
        .await?;
        Ok(verify_response.into())
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, X402SchemeFacilitatorError> {
        let settle_request = types::SettleRequest::try_from(request)?;
        // FIXME
        // For now, settle the full authorized amount
        // In a real implementation, this would be determined by resource consumption
        let settle_response = permit2::settle_permit2_payment(
            &self.provider,
            &settle_request.payment_payload,
            &settle_request.payment_requirements,
            None, // None means use the full authorized amount
        )
        .await?;
        Ok(settle_response.into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError> {
        let chain_id = self.provider.chain_id();
        let kinds = vec![proto::SupportedPaymentKind {
            x402_version: v2::X402Version2.into(),
            scheme: types::UptoScheme.to_string(),
            network: chain_id.clone().into(),
            extra: None,
        }];
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
