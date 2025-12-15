mod chain_id;
pub mod eip155;
pub mod solana;

pub use chain_id::*;

use std::collections::HashMap;

use crate::config::ChainConfig;

#[derive(Debug)]
pub enum ChainProvider {
    Eip155(eip155::Eip155ChainProvider),
    Solana(solana::SolanaChainProvider),
}

impl ChainProvider {
    pub async fn from_config(config: &ChainConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let provider = match config {
            ChainConfig::Eip155(config) => {
                let provider = eip155::Eip155ChainProvider::from_config(config).await?;
                ChainProvider::Eip155(provider)
            }
            ChainConfig::Solana(config) => {
                let provider = solana::SolanaChainProvider::from_config(config).await?;
                ChainProvider::Solana(provider)
            }
        };
        Ok(provider)
    }
}

pub trait ChainProviderOps {
    fn signer_addresses(&self) -> Vec<Box<str>>;
    fn chain_id(&self) -> ChainId;
}

impl ChainProviderOps for ChainProvider {
    fn signer_addresses(&self) -> Vec<Box<str>> {
        match self {
            ChainProvider::Eip155(provider) => provider.signer_addresses(),
            ChainProvider::Solana(provider) => provider.signer_addresses(),
        }
    }

    fn chain_id(&self) -> ChainId {
        match self {
            ChainProvider::Eip155(provider) => provider.chain_id(),
            ChainProvider::Solana(provider) => provider.chain_id(),
        }
    }
}

#[derive(Debug)]
pub struct ChainProviders(HashMap<ChainId, ChainProvider>);

impl ChainProviders {
    pub async fn from_config(
        chains: &Vec<ChainConfig>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut providers = HashMap::new();
        for chain in chains {
            let chain_provider = ChainProvider::from_config(chain).await?;
            providers.insert(chain_provider.chain_id(), chain_provider);
        }
        Ok(Self(providers))
    }
}
