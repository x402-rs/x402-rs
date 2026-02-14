//! Scheme builder implementations for the x402 facilitator.
//!
//! This module provides [`X402SchemeFacilitatorBuilder`] implementations for all supported
//! payment schemes. These builders create scheme facilitators from the generic
//! [`ChainProvider`] enum by extracting the appropriate
//! chain-specific provider.
//!
//! # Supported Schemes
//!
//! | Scheme            | Chains        | Description                                    |
//! |-------------------|---------------|------------------------------------------------|
//! | [`V1Eip155Exact`] | EIP-155 (EVM) | V1 protocol with exact amount on EVM           |
//! | [`V1SolanaExact`] | Solana        | V1 protocol with exact amount on Solana        |
//! | [`V2Eip155Exact`] | EIP-155 (EVM) | V2 protocol with exact amount on EVM           |
//! | [`V2Eip155Upto`]  | EIP-155 (EVM) | V2 protocol with server-selected amount on EVM |
//! | [`V2SolanaExact`] | Solana        | V2 protocol with exact amount on Solana        |
//! | [`V2AptosExact`]  | Aptos         | V2 protocol with exact amount on Aptos         |
//!
//! # Example
//!
//! ```ignore
//! use x402_types::scheme::{SchemeBlueprints, X402SchemeFacilitatorBuilder};
//! use x402_chain_eip155::V2Eip155Exact;
//! use crate::chain::ChainProvider;
//!
//! // Register schemes
//! let blueprints = SchemeBlueprints::new()
//!     .and_register(V2Eip155Exact)
//!     .and_register(V2SolanaExact);
//! ```

#[allow(unused_imports)] // For when no chain features are enabled
use crate::chain::ChainProvider;
#[allow(unused_imports)] // For when no chain features are enabled
use std::sync::Arc;
#[allow(unused_imports)] // For when no chain features are enabled
use x402_types::scheme::{X402SchemeFacilitator, X402SchemeFacilitatorBuilder};

#[cfg(feature = "chain-aptos")]
use x402_chain_aptos::V2AptosExact;
#[cfg(feature = "chain-eip155")]
use x402_chain_eip155::{V1Eip155Exact, V2Eip155Exact, V2Eip155Upto};
#[cfg(feature = "chain-solana")]
use x402_chain_solana::{V1SolanaExact, V2SolanaExact};

#[cfg(feature = "chain-solana")]
impl X402SchemeFacilitatorBuilder<&ChainProvider> for V1SolanaExact {
    fn build(
        &self,
        provider: &ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        #[allow(irrefutable_let_patterns)] // For when just chain-aptos is enabled
        let solana_provider = if let ChainProvider::Solana(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V1SolanaExact::build: provider must be a SolanaChainProvider".into());
        };
        self.build(solana_provider, config)
    }
}

#[cfg(feature = "chain-solana")]
impl X402SchemeFacilitatorBuilder<&ChainProvider> for V2SolanaExact {
    fn build(
        &self,
        provider: &ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        #[allow(irrefutable_let_patterns)] // For when just chain-aptos is enabled
        let solana_provider = if let ChainProvider::Solana(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V2SolanaExact::build: provider must be a SolanaChainProvider".into());
        };
        self.build(solana_provider, config)
    }
}

#[cfg(feature = "chain-eip155")]
impl X402SchemeFacilitatorBuilder<&ChainProvider> for V2Eip155Exact {
    fn build(
        &self,
        provider: &ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        #[allow(irrefutable_let_patterns)] // For when just chain-aptos is enabled
        let eip155_provider = if let ChainProvider::Eip155(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V2Eip155Exact::build: provider must be an Eip155ChainProvider".into());
        };
        self.build(eip155_provider, config)
    }
}

#[cfg(feature = "chain-eip155")]
impl X402SchemeFacilitatorBuilder<&ChainProvider> for V2Eip155Upto {
    fn build(
        &self,
        provider: &ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        #[allow(irrefutable_let_patterns)] // For when just chain-aptos is enabled
        let eip155_provider = if let ChainProvider::Eip155(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V2Eip155Exact::build: provider must be an Eip155ChainProvider".into());
        };
        self.build(eip155_provider, config)
    }
}

#[cfg(feature = "chain-aptos")]
impl X402SchemeFacilitatorBuilder<&ChainProvider> for V2AptosExact {
    fn build(
        &self,
        provider: &ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        #[allow(irrefutable_let_patterns)] // For when just chain-aptos is enabled
        let aptos_provider = if let ChainProvider::Aptos(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V2AptosExact::build: provider must be an AptosChainProvider".into());
        };
        self.build(aptos_provider, config)
    }
}

#[cfg(feature = "chain-eip155")]
impl X402SchemeFacilitatorBuilder<&ChainProvider> for V1Eip155Exact {
    fn build(
        &self,
        provider: &ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        #[allow(irrefutable_let_patterns)] // For when just chain-aptos is enabled
        let eip155_provider = if let ChainProvider::Eip155(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V1Eip155Exact::build: provider must be an Eip155ChainProvider".into());
        };
        self.build(eip155_provider, config)
    }
}
