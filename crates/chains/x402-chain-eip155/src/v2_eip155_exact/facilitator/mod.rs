//! Facilitator-side payment verification and settlement for V2 EIP-155 exact scheme.
//!
//! This module implements the facilitator logic for V2 protocol payments on EVM chains.
//! It reuses most of the V1 verification and settlement logic but handles V2-specific
//! payload structures with embedded requirements and CAIP-2 chain IDs.

pub mod eip3009;
pub mod permit2;

use alloy_provider::Provider;
use std::collections::HashMap;
use x402_types::chain::ChainProviderOps;
use x402_types::proto;
use x402_types::proto::v2;
use x402_types::scheme::{
    X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError,
};

use crate::V2Eip155Exact;
use crate::chain::Eip155MetaTransactionProvider;
use crate::v1_eip155_exact::ExactScheme;
use crate::v1_eip155_exact::facilitator::Eip155ExactError;
use crate::v2_eip155_exact::types;

impl<P> X402SchemeFacilitatorBuilder<P> for V2Eip155Exact
where
    P: Eip155MetaTransactionProvider + ChainProviderOps + Send + Sync + 'static,
    Eip155ExactError: From<P::Error>,
{
    fn build(
        &self,
        provider: P,
        _config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        Ok(Box::new(V2Eip155ExactFacilitator::new(provider)))
    }
}

/// Facilitator for V2 EIP-155 exact scheme payments.
///
/// This struct implements the [`X402SchemeFacilitator`] trait to provide payment
/// verification and settlement services for ERC-3009 based payments on EVM chains
/// using the V2 protocol.
///
/// # Type Parameters
///
/// - `P`: The provider type, which must implement [`Eip155MetaTransactionProvider`]
///   and [`ChainProviderOps`]
pub struct V2Eip155ExactFacilitator<P> {
    provider: P,
}

impl<P> V2Eip155ExactFacilitator<P> {
    /// Creates a new V2 EIP-155 exact scheme facilitator with the given provider.
    pub fn new(provider: P) -> Self {
        Self { provider }
    }
}

#[async_trait::async_trait]
impl<P> X402SchemeFacilitator for V2Eip155ExactFacilitator<P>
where
    P: Eip155MetaTransactionProvider + ChainProviderOps + Send + Sync,
    P::Inner: Provider,
    Eip155ExactError: From<P::Error>,
{
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError> {
        let verify_request = types::FacilitatorVerifyRequest::try_from(request.clone())?;
        let verify_response = match verify_request {
            types::FacilitatorVerifyRequest::Eip3009 {
                payment_payload,
                payment_requirements,
                x402_version: _,
            } => {
                eip3009::verify_eip3009_payment(
                    &self.provider,
                    &payment_payload,
                    &payment_requirements,
                )
                .await?
            }
            types::FacilitatorVerifyRequest::Permit2 {
                payment_requirements,
                payment_payload,
                x402_version: _,
            } => {
                permit2::verify_permit2_payment(
                    &self.provider,
                    &payment_payload,
                    &payment_requirements,
                )
                .await?
            }
        };
        Ok(verify_response.into())
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, X402SchemeFacilitatorError> {
        let settle_request = types::FacilitatorSettleRequest::try_from(request.clone())?;
        let settle_response = match settle_request {
            types::FacilitatorSettleRequest::Eip3009 {
                payment_payload,
                payment_requirements,
                x402_version: _,
            } => {
                eip3009::settle_eip3009_payment(
                    &self.provider,
                    &payment_payload,
                    &payment_requirements,
                )
                .await?
            }
            types::FacilitatorSettleRequest::Permit2 {
                payment_requirements,
                payment_payload,
                x402_version: _,
            } => {
                permit2::settle_permit2_payment(
                    &self.provider,
                    &payment_payload,
                    &payment_requirements,
                )
                .await?
            }
        };
        Ok(settle_response.into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError> {
        let chain_id = self.provider.chain_id();
        let kinds = vec![proto::SupportedPaymentKind {
            x402_version: v2::X402Version2.into(),
            scheme: ExactScheme.to_string(),
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
