use once_cell::sync::Lazy;
use std::collections::HashMap;

use crate::chain::ChainId;

/// A known network definition with its chain ID and human-readable name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NetworkInfo {
    /// Human-readable network name (e.g., "base-sepolia", "solana")
    pub name: &'static str,
    /// CAIP-2 namespace (e.g., "eip155", "solana")
    pub namespace: &'static str,
    /// Chain reference (e.g., "84532" for Base Sepolia, "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp" for Solana mainnet)
    pub reference: &'static str,
}

impl NetworkInfo {
    /// Create a ChainId from this network info
    pub fn chain_id(&self) -> ChainId {
        ChainId::new(self.namespace, self.reference)
    }
}

static KNOWN_NETWORKS: &[NetworkInfo] = &[
    // EVM Networks
    // Base
    NetworkInfo {
        name: "base",
        namespace: "eip155",
        reference: "8453",
    },
    NetworkInfo {
        name: "base-sepolia",
        namespace: "eip155",
        reference: "84532",
    },
    NetworkInfo {
        name: "skale-base-sepolia",
        namespace: "eip155",
        reference: "324705682",
    },
    // Polygon
    NetworkInfo {
        name: "polygon",
        namespace: "eip155",
        reference: "137",
    },
    NetworkInfo {
        name: "polygon-amoy",
        namespace: "eip155",
        reference: "80002",
    },
    // Avalanche
    NetworkInfo {
        name: "avalanche",
        namespace: "eip155",
        reference: "43114",
    },
    NetworkInfo {
        name: "avalanche-fuji",
        namespace: "eip155",
        reference: "43113",
    },
    // Sei
    NetworkInfo {
        name: "sei",
        namespace: "eip155",
        reference: "1329",
    },
    NetworkInfo {
        name: "sei-testnet",
        namespace: "eip155",
        reference: "1328",
    },
    // Abstract
    NetworkInfo {
        name: "abstract",
        namespace: "eip155",
        reference: "2741",
    },
    NetworkInfo {
        name: "abstract-testnet",
        namespace: "eip155",
        reference: "11124",
    },
    // XDC
    NetworkInfo {
        name: "xdc",
        namespace: "eip155",
        reference: "50",
    },
    // XRPL EVM
    NetworkInfo {
        name: "xrpl-evm",
        namespace: "eip155",
        reference: "1440000",
    },
    // Peaq
    NetworkInfo {
        name: "peaq",
        namespace: "eip155",
        reference: "3338",
    },
    // IoTeX
    NetworkInfo {
        name: "iotex",
        namespace: "eip155",
        reference: "4689",
    },
    // Story
    NetworkInfo {
        name: "story",
        namespace: "eip155",
        reference: "1514",
    },
    // Educhain
    NetworkInfo {
        name: "educhain",
        namespace: "eip155",
        reference: "41923",
    },
    // Celo
    NetworkInfo {
        name: "celo",
        namespace: "eip155",
        reference: "42220",
    },
    NetworkInfo {
        name: "celo-alfajores",
        namespace: "eip155",
        reference: "44787",
    },
    // BSC (Binance Smart Chain)
    NetworkInfo {
        name: "bsc",
        namespace: "eip155",
        reference: "56",
    },
    NetworkInfo {
        name: "bsc-testnet",
        namespace: "eip155",
        reference: "97",
    },
    // Monad
    NetworkInfo {
        name: "monad",
        namespace: "eip155",
        reference: "143",
    },
    NetworkInfo {
        name: "monad-testnet",
        namespace: "eip155",
        reference: "10143",
    },
    // Solana Networks
    NetworkInfo {
        name: "solana",
        namespace: "solana",
        reference: "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
    },
    NetworkInfo {
        name: "solana-devnet",
        namespace: "solana",
        reference: "EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
    },
];

/// Lazy hashmap: network name -> NetworkInfo
static NAME_TO_CHAIN_ID: Lazy<HashMap<&'static str, ChainId>> = Lazy::new(|| {
    KNOWN_NETWORKS
        .iter()
        .map(|n| (n.name, n.chain_id()))
        .collect()
});

/// Lazy hashmap: ChainId -> NetworkInfo
static CHAIN_ID_TO_NAME: Lazy<HashMap<ChainId, &'static str>> = Lazy::new(|| {
    KNOWN_NETWORKS
        .iter()
        .map(|n| (n.chain_id(), n.name))
        .collect()
});

pub fn chain_id_by_network_name(name: &str) -> Option<&ChainId> {
    NAME_TO_CHAIN_ID.get(name)
}

pub fn network_name_by_chain_id(chain_id: &ChainId) -> Option<&'static str> {
    CHAIN_ID_TO_NAME.get(chain_id).copied()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_networks_by_name() {
        let base = chain_id_by_network_name("base").unwrap();
        assert_eq!(base.namespace, "eip155");
        assert_eq!(base.reference, "8453");

        let base_sepolia = chain_id_by_network_name("base-sepolia").unwrap();
        assert_eq!(base_sepolia.namespace, "eip155");
        assert_eq!(base_sepolia.reference, "84532");

        let solana = chain_id_by_network_name("solana").unwrap();
        assert_eq!(solana.namespace, "solana");
        assert_eq!(solana.reference, "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");

        assert!(chain_id_by_network_name("unknown-network").is_none());
    }

    #[test]
    fn test_known_networks_by_chain_id() {
        let chain_id = ChainId::new("eip155", "8453");
        let network_name = network_name_by_chain_id(&chain_id).unwrap();
        assert_eq!(network_name, "base");

        let solana_chain_id = ChainId::new("solana", "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
        let network_name = network_name_by_chain_id(&solana_chain_id).unwrap();
        assert_eq!(network_name, "solana");

        let unknown_chain_id = ChainId::new("eip155", "999999");
        assert!(network_name_by_chain_id(&unknown_chain_id).is_none());
    }

    #[test]
    fn test_chain_id_from_network_name() {
        let chain_id = ChainId::from_network_name("base").unwrap();
        assert_eq!(chain_id.namespace, "eip155");
        assert_eq!(chain_id.reference, "8453");

        let solana_chain_id = ChainId::from_network_name("solana").unwrap();
        assert_eq!(solana_chain_id.namespace, "solana");
        assert_eq!(
            solana_chain_id.reference,
            "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"
        );

        assert!(ChainId::from_network_name("unknown").is_none());
    }

    #[test]
    fn test_chain_id_as_network_name() {
        let chain_id = ChainId::new("eip155", "8453");
        assert_eq!(chain_id.as_network_name(), Some("base"));

        let solana_chain_id = ChainId::new("solana", "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
        assert_eq!(solana_chain_id.as_network_name(), Some("solana"));

        let unknown_chain_id = ChainId::new("eip155", "999999");
        assert!(unknown_chain_id.as_network_name().is_none());
    }

    #[test]
    fn test_network_info_chain_id() {
        let chain_id = chain_id_by_network_name("polygon").unwrap();
        assert_eq!(chain_id.namespace, "eip155");
        assert_eq!(chain_id.reference, "137");
    }
}
