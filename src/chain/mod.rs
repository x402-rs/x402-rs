use std::time::SystemTimeError;

pub mod chain_id;
pub mod evm;
pub mod namespace;
pub mod solana;

pub use namespace::*;

use crate::chain::evm::EvmProvider;
use crate::chain::solana::SolanaProvider;
use crate::config::ChainConfig;
use crate::facilitator::Facilitator;
use crate::network::ChainIdToNetworkError;
use crate::p1::chain::ChainId;
use crate::p1::proto;
use crate::types::{
    MixedAddress, SettleRequest, SettleResponse, SupportedResponse, VerifyRequest, VerifyResponse,
};

pub enum NetworkProvider {
    Evm(EvmProvider),
    Solana(SolanaProvider),
}

impl NetworkProvider {
    pub async fn from_config(config: &ChainConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let provider = match config {
            ChainConfig::Eip155(config) => {
                let provider = EvmProvider::from_config(config).await?;
                NetworkProvider::Evm(provider)
            }
            ChainConfig::Solana(config) => {
                let provider = SolanaProvider::from_config(config).await?;
                NetworkProvider::Solana(provider)
            }
        };
        Ok(provider)
    }

    pub fn chain_id(&self) -> ChainId {
        match self {
            NetworkProvider::Evm(provider) => provider.chain_id(),
            NetworkProvider::Solana(provider) => provider.chain_id(),
        }
    }
}

pub trait NetworkProviderOps {
    fn signer_addresses(&self) -> Vec<String>;
    fn chain_id(&self) -> ChainId;
}

impl NetworkProviderOps for NetworkProvider {
    fn signer_addresses(&self) -> Vec<String> {
        match self {
            NetworkProvider::Evm(provider) => provider.signer_addresses(),
            NetworkProvider::Solana(provider) => provider.signer_addresses(),
        }
    }

    fn chain_id(&self) -> ChainId {
        match self {
            NetworkProvider::Evm(provider) => provider.chain_id(),
            NetworkProvider::Solana(provider) => provider.chain_id(),
        }
    }
}

impl Facilitator for NetworkProvider {
    type Error = FacilitatorLocalError;

    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, Self::Error> {
        match self {
            NetworkProvider::Evm(provider) => provider.verify(request).await,
            NetworkProvider::Solana(provider) => provider.verify(request).await,
        }
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, Self::Error> {
        match self {
            NetworkProvider::Evm(provider) => provider.settle(request).await,
            NetworkProvider::Solana(provider) => provider.settle(request).await,
        }
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, Self::Error> {
        match self {
            NetworkProvider::Evm(provider) => provider.supported().await,
            NetworkProvider::Solana(provider) => provider.supported().await,
        }
    }
}

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
