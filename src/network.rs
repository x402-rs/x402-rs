use alloy::primitives::address;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};

#[derive(Debug, Hash, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Network {
    #[serde(rename = "base-sepolia")]
    BaseSepolia,
    #[serde(rename = "base")]
    Base,
}

impl Display for Network {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Network::BaseSepolia => {
                write!(f, "base-sepolia")
            }
            Network::Base => {
                write!(f, "base")
            }
        }
    }
}

impl Network {
    pub fn chain_id(&self) -> u64 {
        match self {
            Network::BaseSepolia => 84532,
            Network::Base => 8453,
        }
    }

    pub fn variants() -> &'static [Network] {
        &[Network::BaseSepolia, Network::Base]
    }
}

pub struct USDCDeployment {
    #[allow(dead_code)]
    pub address: alloy::primitives::Address,
    pub name: String,
}

impl USDCDeployment {
    pub fn by_network(network: &Network) -> USDCDeployment {
        match network {
            Network::BaseSepolia => USDCDeployment {
                address: address!("0x036CbD53842c5426634e7929541eC2318f3dCF7e"),
                name: "USDC".to_string(),
            },
            Network::Base => USDCDeployment {
                address: address!("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
                name: "USDC".to_string(),
            },
        }
    }
}
