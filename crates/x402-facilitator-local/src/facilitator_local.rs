//! Local facilitator implementation for x402 payments.
//!
//! This module provides [`FacilitatorLocal`], a [`Facilitator`](x402_types::facilitator::Facilitator) implementation that
//! validates x402 payment payloads and performs on-chain settlements using the
//! registered scheme handlers.
//!
//! # Architecture
//!
//! The local facilitator delegates payment processing to scheme handlers registered
//! in a [`SchemeRegistry`](x402_types::scheme::SchemeRegistry). Each handler is responsible for:
//!
//! - Verifying payment signatures and requirements
//! - Checking on-chain balances
//! - Executing settlement transactions
//!
//! # Example
//!
//! ```ignore
//! use x402_facilitator_local::FacilitatorLocal;
//! use x402_types::scheme::SchemeRegistry;
//!
//! let registry = SchemeRegistry::build(chain_registry, scheme_blueprints, &config);
//! let facilitator = FacilitatorLocal::new(registry);
//! ```
//!
//! # Scheme Routing
//!
//! The facilitator routes requests to the appropriate scheme handler based on the
//! payment's chain ID and scheme name. The scheme handler slug is extracted from
//! the request and used to look up the handler in the registry.
//!
//! If no matching handler is found, the request returns an error with
//! [`PaymentVerificationError::UnsupportedScheme`](x402_types::proto::PaymentVerificationError::UnsupportedScheme).

use std::collections::HashMap;
use x402_types::facilitator::Facilitator;
use x402_types::proto;
use x402_types::proto::PaymentVerificationError;
use x402_types::scheme::{SchemeRegistry, X402SchemeFacilitatorError};

/// A local [`Facilitator`](x402_types::facilitator::Facilitator) implementation that delegates to scheme handlers.
///
/// This type wraps a [`SchemeRegistry`](x402_types::scheme::SchemeRegistry) and routes payment verification and
/// settlement requests to the appropriate scheme handler based on the payment's
/// chain ID and scheme name.
///
/// # Type Parameter
///
/// - `A` - The handler registry type (typically [`SchemeRegistry`](x402_types::scheme::SchemeRegistry))
///
/// # Example
///
/// ```ignore
/// use x402_facilitator_local::FacilitatorLocal;
/// use x402_types::scheme::SchemeRegistry;
///
/// let scheme_registry = SchemeRegistry::build(chain_registry, scheme_blueprints, &config);
/// let facilitator = FacilitatorLocal::new(scheme_registry);
///
/// // Use the facilitator to verify payments
/// let response = facilitator.verify(&verify_request).await?;
/// ```
pub struct FacilitatorLocal<A> {
    handlers: A,
}

impl<A> FacilitatorLocal<A> {
    /// Creates a new [`FacilitatorLocal`] with the given scheme handler registry.
    ///
    /// # Arguments
    ///
    /// - `handlers` - The scheme registry containing all registered payment handlers
    ///
    /// # Example
    ///
    /// ```ignore
    /// use x402_facilitator_local::FacilitatorLocal;
    /// use x402_types::scheme::SchemeRegistry;
    ///
    /// let scheme_registry = SchemeRegistry::build(chain_registry, scheme_blueprints, &config);
    /// let facilitator = FacilitatorLocal::new(scheme_registry);
    /// ```
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

/// Errors that can occur during local facilitator operations.
///
/// These errors wrap the underlying scheme handler errors and distinguish between
/// verification failures (which occur during the `/verify` step) and settlement
/// failures (which occur during the `/settle` step).
#[derive(Debug, thiserror::Error)]
pub enum FacilitatorLocalError {
    /// Payment verification failed.
    ///
    /// This error occurs when the scheme handler fails to verify a payment,
    /// typically due to invalid signatures, unsupported schemes, or insufficient funds.
    #[error(transparent)]
    Verification(X402SchemeFacilitatorError),
    /// Payment settlement failed.
    ///
    /// This error occurs when the scheme handler fails to settle a payment on-chain,
    /// typically due to transaction failures or network issues.
    #[error(transparent)]
    Settlement(X402SchemeFacilitatorError),
}
