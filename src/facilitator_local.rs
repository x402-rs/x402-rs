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

use crate::facilitator::Facilitator;
use crate::proto;
use crate::proto::PaymentVerificationError;
use crate::scheme::{SchemeRegistry, X402SchemeFacilitatorError};

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

    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, Self::Error> {
        let handler = request
            .scheme_handler_slug()
            .and_then(|slug| self.handlers.by_slug(&slug))
            .ok_or(FacilitatorLocalError::Verification(
                PaymentVerificationError::UnsupportedScheme.into(),
            ))?;
        let response = handler
            .verify(request)
            .await
            .map_err(FacilitatorLocalError::Verification)?;
        Ok(response)
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, Self::Error> {
        let handler = request
            .scheme_handler_slug()
            .and_then(|slug| self.handlers.by_slug(&slug))
            .ok_or(FacilitatorLocalError::Verification(
                PaymentVerificationError::UnsupportedScheme.into(),
            ))?;
        let response = handler
            .settle(request)
            .await
            .map_err(FacilitatorLocalError::Settlement)?;
        Ok(response)
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, Self::Error> {
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
        Ok(proto::SupportedResponse {
            kinds,
            extensions: Vec::new(),
            signers,
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum FacilitatorLocalError {
    #[error(transparent)]
    Verification(X402SchemeFacilitatorError),
    #[error(transparent)]
    Settlement(X402SchemeFacilitatorError),
}
