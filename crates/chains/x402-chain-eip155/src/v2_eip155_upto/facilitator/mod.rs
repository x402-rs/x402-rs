//! Facilitator-side payment verification and settlement for V2 EIP-155 upto scheme.
//!
//! This module implements the facilitator logic for V2 protocol "upto" payments on EVM chains.
//! The upto scheme allows clients to authorize a maximum amount, with the actual settled amount
//! determined by the server at settlement time based on resource consumption.

pub mod eip2612;
pub mod permit2;

use alloy_provider::Provider;
use rand::seq::IndexedRandom;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use x402_types::chain::ChainProviderOps;
use x402_types::proto;
use x402_types::proto::v2;
use x402_types::scheme::{
    ExtensionKey, X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError,
};

use crate::V2Eip155Upto;
use crate::chain::{Eip155MetaTransactionProvider, Eip155SignerAddresses};
use crate::eip2612_gas_sponsoring::Eip2612GasSponsoring;
use crate::v1_eip155_exact::facilitator::Eip155ExactError;
use crate::v2_eip155_upto::types;

/// Configuration for the V2 EIP-155 upto scheme facilitator.
///
/// - `eip2612_gas_sponsoring`: Whether to enable EIP-2612 gas-sponsoring extension.
///   When enabled, the facilitator supports atomic settlement with EIP-2612 permits,
///   allowing the payer to have their gas fees covered by the facilitator.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct V2Eip155UptoFacilitatorConfig {
    #[serde(default)]
    pub eip2612_gas_sponsoring: bool,
}

impl<P> X402SchemeFacilitatorBuilder<P> for V2Eip155Upto
where
    P: Eip155MetaTransactionProvider
        + ChainProviderOps
        + Eip155SignerAddresses
        + Send
        + Sync
        + 'static,
    Eip155ExactError: From<P::Error>,
{
    fn build(
        &self,
        provider: P,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let config: V2Eip155UptoFacilitatorConfig = config
            .and_then(|c| serde_json::from_value(c).ok())
            .unwrap_or_default();
        Ok(Box::new(V2Eip155UptoFacilitator::new(provider, config)))
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
    eip2612_gas_sponsoring: bool,
}

impl<P> V2Eip155UptoFacilitator<P> {
    pub fn new(provider: P, config: V2Eip155UptoFacilitatorConfig) -> Self {
        Self {
            provider,
            eip2612_gas_sponsoring: config.eip2612_gas_sponsoring,
        }
    }
}

#[async_trait::async_trait]
impl<P> X402SchemeFacilitator for V2Eip155UptoFacilitator<P>
where
    P: Eip155MetaTransactionProvider + ChainProviderOps + Eip155SignerAddresses + Send + Sync,
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
            self.eip2612_gas_sponsoring,
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
        let settle_response = permit2::settle_permit2_payment(
            &self.provider,
            self.eip2612_gas_sponsoring,
            &settle_request.payment_payload,
            &settle_request.payment_requirements,
        )
        .await?;
        Ok(settle_response.into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError> {
        let chain_id = self.provider.chain_id();

        let mut extensions = vec![];
        if self.eip2612_gas_sponsoring {
            extensions.push(Eip2612GasSponsoring::EXTENSION_KEY.to_string());
        }

        let signer_addresses = Eip155SignerAddresses::signer_addresses(&self.provider);
        let mut rng = rand::rng();
        let facilitator_address = signer_addresses.choose(&mut rng);
        let extra = facilitator_address
            .map(|addr| types::UptoSupportedExtra {
                facilitator_address: addr,
                extensions: extensions.clone(),
            })
            .and_then(|extra| serde_json::to_value(extra).ok());

        let kinds = vec![proto::SupportedPaymentKind {
            x402_version: v2::X402Version2.into(),
            scheme: types::UptoScheme.to_string(),
            network: chain_id.clone().into(),
            extra,
        }];
        let signers = {
            let mut signers = HashMap::with_capacity(1);
            let signer_addresses = ChainProviderOps::signer_addresses(&self.provider);
            signers.insert(chain_id, signer_addresses);
            signers
        };
        Ok(proto::SupportedResponse {
            kinds,
            extensions,
            signers,
        })
    }
}
