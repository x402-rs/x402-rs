use std::time::SystemTimeError;

pub mod namespace;

pub use namespace::*;

use crate::facilitator::Facilitator;
use crate::network::ChainIdToNetworkError;
use crate::types::{
    MixedAddress,
};

#[derive(Debug, thiserror::Error)]
pub enum FacilitatorLocalError {
    /// The network is not supported by this facilitator.
    #[error("Unsupported network")]
    UnsupportedNetwork(Option<MixedAddress>),
    /// The network is not supported by this facilitator.
    #[error("Network mismatch: expected {1}, actual {2}")]
    NetworkMismatch(Option<MixedAddress>, String, String),
    /// Scheme mismatch.
    #[error("Scheme mismatch: expected {1}, actual {2}")]
    SchemeMismatch(Option<MixedAddress>, String, String),
    /// Invalid address.
    #[error("Invalid address: {0}")]
    InvalidAddress(String),
    /// The `pay_to` recipient in the requirements doesn't match the `to` address in the payload.
    #[error("Incompatible payload receivers (payload: {1}, requirements: {2})")]
    ReceiverMismatch(MixedAddress, String, String),
    /// Failed to read a system clock to check timing.
    #[error("Can not get system clock")]
    ClockError(#[source] SystemTimeError),
    /// The `validAfter`/`validBefore` fields on the authorization are not within bounds.
    #[error("Invalid timing: {1}")]
    InvalidTiming(MixedAddress, String),
    /// Low-level contract interaction failure (e.g. call failed, method not found).
    #[error("Invalid contract call: {0}")]
    ContractCall(String),
    /// EIP-712 signature is invalid or mismatched.
    #[error("Invalid signature: {1}")]
    InvalidSignature(MixedAddress, String),
    /// The payer's on-chain balance is insufficient for the payment.
    #[error("Insufficient funds")]
    InsufficientFunds(MixedAddress),
    /// The payload's `value` is not enough to meet the requirements.
    #[error("Insufficient value")]
    InsufficientValue(MixedAddress),
    /// The payload decoding failed.
    #[error("Decoding error: {0}")]
    DecodingError(String),
    #[error("Can not convert chain ID to network")]
    NetworkConversionError(#[source] ChainIdToNetworkError),
}
