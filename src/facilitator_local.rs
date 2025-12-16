//! Facilitator implementation for x402 payments using on-chain verification and settlement.
//!
//! This module provides a [`Facilitator`] implementation that validates x402 payment payloads
//! and performs on-chain settlements using ERC-3009 `transferWithAuthorization`.
//!
//! Features include:
//! - EIP-712 signature recovery
//! - ERC-20 balance checks
//! - Contract interaction using Alloy
//! - Network-specific configuration via [`ProviderCache`] and [`USDCDeployment`]

use std::collections::HashMap;
use tracing::instrument;

use crate::chain::FacilitatorLocalError;
use crate::facilitator::Facilitator;
use crate::p1::scheme::SchemeRegistry;
use crate::provider_cache::ProviderMap;
use crate::types::{
    SettleRequest, SettleResponse, SupportedResponse, VerifyRequest, VerifyResponse,
};

/// A concrete [`Facilitator`] implementation that verifies and settles x402 payments
/// using a network-aware provider cache.
///
/// This type is generic over the [`ProviderMap`] implementation used to access EVM providers,
/// which enables testing or customization beyond the default [`ProviderCache`].
pub struct FacilitatorLocal<A> {
    handlers: A,
}

impl<A> FacilitatorLocal<A> {
    /// Creates a new [`FacilitatorLocal`] with the given provider cache.
    ///
    /// The provider cache is used to resolve the appropriate EVM provider for each payment's target network.
    pub fn new(handlers: A) -> Self {
        FacilitatorLocal { handlers }
    }
}

impl Facilitator for FacilitatorLocal<SchemeRegistry> {
    type Error = FacilitatorLocalError;

    async fn verify(&self, request: &VerifyRequest) -> Result<VerifyResponse, Self::Error> {
        todo!()
    }

    async fn settle(&self, request: &SettleRequest) -> Result<SettleResponse, Self::Error> {
        todo!()
    }

    async fn supported(&self) -> Result<SupportedResponse, Self::Error> {
        todo!()
    }
}

impl<A, E> Facilitator for FacilitatorLocal<A>
where
    A: ProviderMap + Sync,
    A::Value: Facilitator<Error = E>,
    E: Send,
    FacilitatorLocalError: From<E>,
{
    type Error = FacilitatorLocalError;

    /// Verifies a proposed x402 payment payload against a passed [`PaymentRequirements`].
    ///
    /// This function validates the signature, timing, receiver match, network, scheme, and on-chain
    /// balance sufficiency for the token. If all checks pass, return a [`VerifyResponse::Valid`].
    ///
    /// Called from the `/verify` HTTP endpoint on the facilitator.
    ///
    /// # Errors
    ///
    /// Returns [`FacilitatorLocalError`] if any check fails, including:
    /// - scheme/network mismatch,
    /// - receiver mismatch,
    /// - invalid signature,
    /// - expired or future-dated timing,
    /// - insufficient funds,
    /// - unsupported network.
    #[instrument(skip_all, err, fields(network = %request.payment_payload.network))]
    async fn verify(&self, request: &VerifyRequest) -> Result<VerifyResponse, Self::Error> {
        let chain_id = request.network().as_chain_id();
        let provider = self
            .handlers
            .by_chain_id(&chain_id)
            .ok_or(FacilitatorLocalError::UnsupportedNetwork(None))?;
        let verify_response = provider.verify(request).await?;
        Ok(verify_response)
    }

    /// Executes an x402 payment on-chain using ERC-3009 `transferWithAuthorization`.
    ///
    /// This function performs the same validations as `verify`, then sends the authorized transfer
    /// via a smart contract and waits for transaction receipt.
    ///
    /// Called from the `/settle` HTTP endpoint on the facilitator.
    ///
    /// # Errors
    ///
    /// Returns [`FacilitatorLocalError`] if validation or contract call fails. Transaction receipt is included
    /// in the response on success or failure.
    #[instrument(skip_all, err, fields(network = %request.payment_payload.network))]
    async fn settle(&self, request: &SettleRequest) -> Result<SettleResponse, Self::Error> {
        let chain_id = request.network().as_chain_id();
        let provider = self
            .handlers
            .by_chain_id(&chain_id)
            .ok_or(FacilitatorLocalError::UnsupportedNetwork(None))?;
        let settle_response = provider.settle(request).await?;
        Ok(settle_response)
    }

    async fn supported(&self) -> Result<SupportedResponse, Self::Error> {
        let mut kinds = vec![];
        let mut signers = HashMap::new();
        for provider in self.handlers.values() {
            let supported = provider.supported().await.ok();
            if let Some(mut supported) = supported {
                kinds.append(&mut supported.kinds);
                for (chain_id, signer_addresses) in supported.signers {
                    signers.entry(chain_id).or_insert(signer_addresses);
                }
            }
        }
        Ok(SupportedResponse {
            kinds,
            extensions: Vec::new(),
            signers,
        })
    }
}
