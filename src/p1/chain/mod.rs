mod chain_id;
pub mod eip155;
pub mod solana;

pub use chain_id::*;

use std::collections::HashMap;

use crate::config::ChainConfig;
use crate::p1::chain::eip155::Eip155ChainProvider;

pub enum ChainProvider {
    Eip155(Eip155ChainProvider),
}

impl ChainProvider {
    pub async fn from_config(config: &ChainConfig) -> Result<Self, Box<dyn std::error::Error>> {
        todo!("ChainProvider::from_config")
    }
}

pub struct ChainProviders(HashMap<ChainId, ChainProvider>);

impl ChainProviders {
    pub async fn from_config(
        chains: &Vec<ChainConfig>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut providers = HashMap::new();
        for chain in chains {
            let chain_provider = ChainProvider::from_config(chain).await?;
            // providers.insert(chain.chain_id(), chain_provider);
        }
        Ok(Self(providers))
    }
}
