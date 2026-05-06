//! Core trait defining the verification and settlement interface for x402 facilitators.
//!
//! Implementors of this trait are responsible for validating incoming payment payloads
//! against specified requirements ([`Facilitator::verify`]) and executing on-chain transfers ([`Facilitator::settle`]).

use std::fmt::{Debug, Display};
use std::future::Future;
use std::sync::Arc;

use crate::proto;

/// Type-level contract that associates the concrete request/response types used
/// by a [`Facilitator`] implementation.
///
/// Implementors supply the concrete message types for each of the three
/// facilitator operations (`verify`, `settle`, `supported`), allowing the
/// [`Facilitator`] trait to be used with different wire formats or test
/// doubles without changing its method signatures.
pub trait FacilitatorContract {
    /// The input type for a verification request.
    type VerifyRequest;
    /// The output type for a verification response.
    type VerifyResponse;
    /// The input type for a settlement request.
    type SettleRequest;
    /// The output type for a settlement response.
    type SettleResponse;
    /// The output type for a supported-schemes response.
    type SupportedResponse;
}

/// The default [`FacilitatorContract`] that uses the canonical x402 types from [`proto`].
///
/// This is the contract used in production; it ties each associated type to the
/// corresponding type in [`proto`].
pub struct ProtoContract;

impl FacilitatorContract for ProtoContract {
    type VerifyRequest = proto::VerifyRequest;
    type VerifyResponse = proto::VerifyResponse;
    type SettleRequest = proto::SettleRequest;
    type SettleResponse = proto::SettleResponse;
    type SupportedResponse = proto::SupportedResponse;
}

/// Trait defining the asynchronous interface for x402 payment facilitators.
///
/// This interface is implemented by any type that performs validation and
/// settlement of payment payloads according to the x402 specification.
pub trait Facilitator<C: FacilitatorContract = ProtoContract> {
    /// The error type returned by this facilitator.
    type Error: Debug + Display;

    /// Verifies a proposed x402 payment payload against a [`proto::VerifyRequest`].
    ///
    /// This includes checking payload integrity, signature validity, balance sufficiency,
    /// network compatibility, and compliance with the declared payment requirements.
    ///
    /// # Returns
    ///
    /// A [`proto::VerifyResponse`] indicating success or failure, wrapped in a [`Result`].
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if any validation step fails.
    fn verify(
        &self,
        request: &C::VerifyRequest,
    ) -> impl Future<Output = Result<C::VerifyResponse, Self::Error>> + Send;

    /// Executes an on-chain x402 settlement for a valid [`proto::SettleRequest`].
    ///
    /// This method should re-validate the payment and, if valid, perform
    /// an onchain call to settle the payment.
    ///
    /// # Returns
    ///
    /// A [`proto::SettleResponse`] indicating whether the settlement was successful, and
    /// containing any on-chain transaction metadata.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if verification or settlement fails.
    fn settle(
        &self,
        request: &C::SettleRequest,
    ) -> impl Future<Output = Result<C::SettleResponse, Self::Error>> + Send;

    /// Returns the payment schemes and networks supported by this facilitator.
    ///
    /// # Returns
    ///
    /// A [`proto::SupportedResponse`] listing the supported scheme/network combinations.
    ///
    /// # Errors
    ///
    /// Returns [`Self::Error`] if the facilitator is unable to enumerate its capabilities.
    #[allow(dead_code)] // For some reason clippy believes it is not used.
    fn supported(&self) -> impl Future<Output = Result<C::SupportedResponse, Self::Error>> + Send;
}

impl<C, T> Facilitator<C> for Arc<T>
where
    C: FacilitatorContract,
    T: Facilitator<C>,
{
    type Error = T::Error;

    fn verify(
        &self,
        request: &C::VerifyRequest,
    ) -> impl Future<Output = Result<C::VerifyResponse, Self::Error>> + Send {
        self.as_ref().verify(request)
    }

    fn settle(
        &self,
        request: &C::SettleRequest,
    ) -> impl Future<Output = Result<C::SettleResponse, Self::Error>> + Send {
        self.as_ref().settle(request)
    }

    fn supported(&self) -> impl Future<Output = Result<C::SupportedResponse, Self::Error>> + Send {
        self.as_ref().supported()
    }
}
