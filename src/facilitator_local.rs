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

use crate::chain::ChainIdFromNetworkNameError;
use crate::chain::eip155::Eip155ChainProviderMetaTransactionError;
use crate::facilitator::Facilitator;
use crate::proto;
use crate::scheme::SchemeRegistry;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::Display;

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
            .ok_or(FacilitatorLocalError::UnsupportedNetwork)?;
        handler.verify(request).await
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, Self::Error> {
        let handler = request
            .scheme_handler_slug()
            .and_then(|slug| self.handlers.by_slug(&slug))
            .ok_or(FacilitatorLocalError::UnsupportedNetwork)?;
        handler.settle(request).await
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
    /// The network is not supported by this facilitator.
    #[error("Unsupported network")]
    UnsupportedNetwork,
    /// The network is not supported by this facilitator.
    #[error("Network mismatch")]
    NetworkMismatch,
    /// Scheme mismatch.
    #[error("Scheme mismatch")]
    SchemeMismatch,
    /// The `pay_to` recipient in the requirements doesn't match the `to` address in the payload.
    #[error("Incompatible payload receivers (payload: {1}, requirements: {2})")]
    ReceiverMismatch(String, String, String),
    /// The `validAfter`/`validBefore` fields on the authorization are not within bounds.
    #[error("Invalid timing: {1}")]
    InvalidTiming(String, String),
    /// Low-level contract interaction failure (e.g. call failed, method not found).
    #[error("Invalid contract call: {0}")]
    ContractCall(String),
    /// EIP-712 signature is invalid or mismatched.
    #[error("Invalid signature: {1}")]
    InvalidSignature(String, String),
    /// The payer's on-chain balance is insufficient for the payment.
    #[error("Insufficient funds")]
    InsufficientFunds(String),
    /// The payload's `value` is not enough to meet the requirements.
    #[error("Insufficient value")]
    InsufficientValue(String),
    /// The payload decoding failed.
    #[error("Decoding error: {0}")]
    DecodingError(String),
}

impl From<Eip155ChainProviderMetaTransactionError> for FacilitatorLocalError {
    fn from(value: Eip155ChainProviderMetaTransactionError) -> Self {
        // TODO ERRORS
        FacilitatorLocalError::ContractCall(value.to_string())
    }
}

impl From<ChainIdFromNetworkNameError> for FacilitatorLocalError {
    fn from(_value: ChainIdFromNetworkNameError) -> Self {
        // TODO ERRORS
        FacilitatorLocalError::UnsupportedNetwork
    }
}

// FIXME ERRORS are fucked up

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
