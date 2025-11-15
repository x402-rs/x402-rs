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

use tracing::instrument;

use crate::chain::FacilitatorLocalError;
use crate::facilitator::Facilitator;
use crate::provider_cache::ProviderMap;
use crate::types::{
    SettleRequest, SettleResponse, SupportedPaymentKindsResponse, VerifyRequest, VerifyResponse,
};

/// A concrete [`Facilitator`] implementation that verifies and settles x402 payments
/// using a network-aware provider cache.
///
/// This type is generic over the [`ProviderMap`] implementation used to access EVM providers,
/// which enables testing or customization beyond the default [`ProviderCache`].
pub struct FacilitatorLocal<A> {
    provider_map: A,
}

impl<A> FacilitatorLocal<A> {
    /// Creates a new [`FacilitatorLocal`] with the given provider cache.
    ///
    /// The provider cache is used to resolve the appropriate EVM provider for each payment's target network.
    pub fn new(provider_map: A) -> Self {
        FacilitatorLocal { provider_map }
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
        let network = request.network();
        let provider = self
            .provider_map
            .by_network(network)
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
        let network = request.network();
        let provider = self
            .provider_map
            .by_network(network)
            .ok_or(FacilitatorLocalError::UnsupportedNetwork(None))?;
        let settle_response = provider.settle(request).await?;
        Ok(settle_response)
    }

    async fn supported(&self) -> Result<SupportedPaymentKindsResponse, Self::Error> {
        let mut kinds = vec![];
        for provider in self.provider_map.values() {
            let supported = provider.supported().await.ok();
            let mut supported_kinds = supported.map(|k| k.kinds).unwrap_or_default();
            kinds.append(&mut supported_kinds);
        }
        Ok(SupportedPaymentKindsResponse { kinds })
    }
}
