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

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::instrument;

use crate::chain::{FacilitatorLocalError, NetworkProvider, NetworkProviderOps};
use crate::facilitator::Facilitator;
use crate::network::Network;
use crate::provider_cache::ProviderCache;
use crate::provider_cache::ProviderMap;
use crate::types::{
    MixedAddress, Scheme, SettleRequest, SettleResponse, SupportedPaymentKind,
    SupportedPaymentKindExtra, SupportedPaymentKindsResponse, VerifyRequest, VerifyResponse,
    X402Version,
};

/// A concrete [`Facilitator`] implementation that verifies and settles x402 payments
/// using a network-aware provider cache.
///
/// This type is generic over the [`ProviderMap`] implementation used to access EVM providers,
/// which enables testing or customization beyond the default [`ProviderCache`].
#[derive(Clone)]
pub struct FacilitatorLocal {
    pub provider_cache: Arc<ProviderCache>,
}

impl FacilitatorLocal {
    /// Creates a new [`FacilitatorLocal`] with the given provider cache.
    ///
    /// The provider cache is used to resolve the appropriate EVM provider for each payment's target network.
    pub fn new(provider_cache: ProviderCache) -> Self {
        FacilitatorLocal {
            provider_cache: Arc::new(provider_cache),
        }
    }

    pub fn kinds(&self) -> Vec<SupportedPaymentKind> {
        self.provider_cache
            .into_iter()
            .map(|(network, provider)| match provider {
                NetworkProvider::Evm(_) => SupportedPaymentKind {
                    x402_version: X402Version::V1,
                    scheme: Scheme::Exact,
                    network: network.to_string(),
                    extra: None,
                },
                NetworkProvider::Solana(provider) => SupportedPaymentKind {
                    x402_version: X402Version::V1,
                    scheme: Scheme::Exact,
                    network: network.to_string(),
                    extra: Some(SupportedPaymentKindExtra {
                        fee_payer: provider.signer_address(),
                    }),
                },
            })
            .collect()
    }

    pub fn health(&self) -> Vec<HealthStatus> {
        self.provider_cache
            .into_iter()
            .map(|(network, provider)| match provider {
                NetworkProvider::Evm(_) => HealthStatus {
                    network: *network,
                    address: provider.signer_address(),
                },
                NetworkProvider::Solana(provider) => HealthStatus {
                    network: *network,
                    address: provider.signer_address(),
                },
            })
            .collect()
    }
}

impl Facilitator for FacilitatorLocal {
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
            .provider_cache
            .by_network(network)
            .ok_or(FacilitatorLocalError::UnsupportedNetwork(None))?;
        provider.verify(request).await
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
            .provider_cache
            .by_network(network)
            .ok_or(FacilitatorLocalError::UnsupportedNetwork(None))?;
        provider.settle(request).await
    }

    async fn supported(&self) -> Result<SupportedPaymentKindsResponse, Self::Error> {
        let kinds = self.kinds();
        Ok(SupportedPaymentKindsResponse { kinds })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthStatus {
    pub network: Network,
    pub address: MixedAddress,
}
