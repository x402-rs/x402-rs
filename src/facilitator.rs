//! Core trait defining the verification and settlement interface for x402 facilitators.
//!
//! Implementors of this trait are responsible for validating incoming payment payloads
//! against specified requirements [`Facilitator::verify`] and executing on-chain transfers [`Facilitator::settle`].

use std::fmt::{Debug, Display};
use std::sync::Arc;

use crate::proto;

/// Trait defining the asynchronous interface for x402 payment facilitators.
///
/// This interface is implemented by any type that performs validation and
/// settlement of payment payloads according to the x402 specification.
pub trait Facilitator {
    /// The error type returned by this facilitator.
    type Error: Debug + Display;

    /// Verifies a proposed x402 payment payload against a [`VerifyRequest`].
    ///
    /// This includes checking payload integrity, signature validity, balance sufficiency,
    /// network compatibility, and compliance with the declared payment requirements.
    ///
    /// # Returns
    ///
    /// A [`VerifyResponse`] indicating success or failure, wrapped in a [`Result`].
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if any validation step fails.
    fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> impl Future<Output = Result<proto::VerifyResponse, Self::Error>> + Send;

    /// Executes an on-chain x402 settlement for a valid [`SettleRequest`].
    ///
    /// This method should re-validate the payment and, if valid, perform
    /// an onchain call to settle the payment.
    ///
    /// # Returns
    ///
    /// A [`SettleResponse`] indicating whether the settlement was successful, and
    /// containing any on-chain transaction metadata.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if verification or settlement fails.
    fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> impl Future<Output = Result<proto::SettleResponse, Self::Error>> + Send;

    #[allow(dead_code)] // For some reason clippy believes it is not used.
    fn supported(
        &self,
    ) -> impl Future<Output = Result<proto::SupportedResponse, Self::Error>> + Send;
}

impl<T: Facilitator> Facilitator for Arc<T> {
    type Error = T::Error;

    fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> impl Future<Output = Result<proto::VerifyResponse, Self::Error>> + Send {
        self.as_ref().verify(request)
    }

    fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> impl Future<Output = Result<proto::SettleResponse, Self::Error>> + Send {
        self.as_ref().settle(request)
    }

    fn supported(
        &self,
    ) -> impl Future<Output = Result<proto::SupportedResponse, Self::Error>> + Send {
        self.as_ref().supported()
    }
}
