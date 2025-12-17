//! Core trait defining the verification and settlement interface for x402 facilitators.
//!
//! Implementors of this trait are responsible for validating incoming payment payloads
//! against specified requirements [`Facilitator::verify`] and executing on-chain transfers [`Facilitator::settle`].

use serde::{Deserialize, Serialize};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorReason {
    // ============================================
    // General Errors
    // ============================================

    /// Required parameters (paymentPayload or paymentRequirements) are missing from the request
    MissingParameters,

    /// An unexpected error occurred during processing
    UnexpectedError,

    /// The payment scheme specified is not valid or supported
    UnsupportedScheme,

    /// An unexpected error occurred during the verify process
    UnexpectedVerifyError,

    // ============================================
    // Transaction State Errors
    // ============================================

    /// The transaction is in an invalid state (e.g., failed or reverted)
    InvalidTransactionState,

    /// The transaction failed before being put onchain
    TransactionFailed,

    // ============================================
    // Balance/Funds Errors
    // ============================================

    /// The payer has insufficient funds to complete the transaction
    InsufficientFunds,

    // ============================================
    // Signature Errors
    // ============================================

    /// The signature is invalid
    InvalidSignature,

    /// The signature has expired
    ExpiredSignature,

    // ============================================
    // EVM-Specific Validation Errors
    // ============================================

    /// The EIP-712 domain is missing from the payload
    MissingEip712Domain,

    /// The permit signature is invalid
    InvalidExactEvmPayloadSignature,

    /// The recipient address doesn't match the expected recipient
    InvalidExactEvmPayloadRecipientMismatch,

    /// The authorization validBefore timestamp is too soon
    /// The deadline on the permit isn't far enough in the future
    InvalidExactEvmPayloadAuthorizationValidBefore,

    /// The authorization validAfter timestamp is in the future
    /// The deadline on the permit is in the future
    InvalidExactEvmPayloadAuthorizationValidAfter,

    /// The authorization value is insufficient to cover the payment requirements
    InvalidExactEvmPayloadAuthorizationValue,

    // ============================================
    // SVM (Solana) Specific Errors
    // ============================================

    /// The transaction amount in the payload doesn't match the expected amount
    InvalidExactSvmPayloadTransactionAmountMismatch,

    /// The SVM payload transaction is invalid or malformed
    InvalidExactSvmPayloadTransaction,

    /// The transaction simulation failed on Solana
    /// This typically indicates the transaction would fail if submitted
    InvalidExactSvmPayloadTransactionSimulationFailed,

    /// The Solana block height has been exceeded
    /// This means the transaction's blockhash is no longer valid
    SettleExactSvmBlockHeightExceeded,

    /// The transaction confirmation timed out on Solana
    /// The transaction may or may not have been processed
    SettleExactSvmTransactionConfirmationTimedOut,

    /// The fee payer is missing from the transaction
    InvalidExactSvmPayloadMissingFeePayer,

    /// The fee payer is not managed by the facilitator
    FeePayerNotManagedByFacilitator,

    /// The transaction could not be decoded
    InvalidExactSvmPayloadTransactionCouldNotBeDecoded,

    /// The transaction has an invalid number of instructions
    InvalidExactSvmPayloadTransactionInstructionsLength,

    /// No transfer instruction was found in the transaction
    InvalidExactSvmPayloadNoTransferInstruction,

    /// The fee payer is attempting to transfer funds (not allowed)
    InvalidExactSvmPayloadTransactionFeePayerTransferringFunds,

    /// The token mint doesn't match the expected mint
    InvalidExactSvmPayloadMintMismatch,

    /// The recipient doesn't match the expected recipient
    InvalidExactSvmPayloadRecipientMismatch,

    /// The transfer amount is insufficient
    InvalidExactSvmPayloadAmountInsufficient,

    // ============================================
    // Settle Specific Errors
    // ============================================

    /// An unexpected error occurred during the settlement process
    UnexpectedSettleError,
}

impl Display for ErrorReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Leverage serde_json to get the snake_case variant name
        let json = serde_json::to_string(self).map_err(|_| std::fmt::Error)?;
        // Remove the surrounding quotes from the JSON string
        write!(f, "{}", json.trim_matches('"'))
    }
}