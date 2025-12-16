mod chain_id;
pub mod eip155;
pub mod solana;

pub use chain_id::*;

use crate::config::ChainConfig;
use std::collections::HashMap;
use std::sync::Arc;

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
    fn signer_addresses(&self) -> Vec<&str>;
    fn chain_id(&self) -> ChainId;
}

impl ChainProviderOps for ChainProvider {
    fn signer_addresses(&self) -> Vec<&str> {
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
pub struct ChainRegistry(HashMap<ChainId, Arc<ChainProvider>>);

impl ChainRegistry {
    pub async fn from_config(
        chains: &Vec<ChainConfig>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut providers = HashMap::new();
        for chain in chains {
            let chain_provider = ChainProvider::from_config(chain).await?;
            providers.insert(chain_provider.chain_id(), Arc::new(chain_provider));
        }
        Ok(Self(providers))
    }

    pub fn by_chain_id(&self, chain_id: ChainId) -> Option<Arc<ChainProvider>> {
        self.0.get(&chain_id).cloned()
    }
}
