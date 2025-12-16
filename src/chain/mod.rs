mod chain_id;
pub mod eip155;
pub mod solana;

pub use chain_id::*;

use crate::config::ChainConfig;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum ChainProvider {
    Eip155(Arc<eip155::Eip155ChainProvider>),
    Solana(Arc<solana::SolanaChainProvider>),
}

impl ChainProvider {
    pub async fn from_config(config: &ChainConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let provider = match config {
            ChainConfig::Eip155(config) => {
                let provider = eip155::Eip155ChainProvider::from_config(config).await?;
                ChainProvider::Eip155(Arc::new(provider))
            }
            ChainConfig::Solana(config) => {
                let provider = solana::SolanaChainProvider::from_config(config).await?;
                ChainProvider::Solana(Arc::new(provider))
            }
        };
        Ok(provider)
    }
}

pub trait ChainProviderOps {
    fn signer_addresses(&self) -> Vec<String>;
    fn chain_id(&self) -> ChainId;
}

impl ChainProviderOps for ChainProvider {
    fn signer_addresses(&self) -> Vec<String> {
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
pub struct ChainRegistry(HashMap<ChainId, ChainProvider>);

impl ChainRegistry {
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

    #[allow(dead_code)]
    pub fn by_chain_id(&self, chain_id: ChainId) -> Option<ChainProvider> {
        self.0.get(&chain_id).cloned()
    }

    pub fn by_chain_id_pattern(&self, pattern: &ChainIdPattern) -> Option<ChainProvider> {
        self.0.iter().find_map(|(chain_id, provider)| {
            if pattern.matches(chain_id) {
                Some(provider.clone())
            } else {
                None
            }
        })
    }
}
