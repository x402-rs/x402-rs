//! Known blockchain networks and CAIP-2 chain ID management.
//!
//! This module provides a comprehensive registry of well-known blockchain networks with their
//! corresponding CAIP-2 (Chain Agnostic Improvement Proposal 2) chain identifiers. It is designed
//! to improve developer experience (DX) when working with the x402 protocol, which operates on
//! CAIP-2 chain IDs.
//!
//! # Purpose
//!
//! This module serves two main purposes:
//! 1. **Compatibility with x402 protocol v1**: Maintains support for networks that were known in v1
//! 2. **Better Developer Experience**: Provides convenient methods to work with well-known networks
//!    without manually constructing CAIP-2 identifiers
//!
//! # CAIP-2 Standard
//!
//! CAIP-2 is a standard for identifying blockchain networks in a chain-agnostic way. A CAIP-2
//! chain ID consists of two parts separated by a colon:
//! - **Namespace**: The blockchain ecosystem (e.g., "eip155" for EVM, "solana" for Solana)
//! - **Reference**: The chain-specific identifier (e.g., "8453" for Base, "137" for Polygon)
//!
//! For more information, see: https://chainagnostic.org/CAIPs/caip-2
//!
//! # Module Contents
//!
//! - [`NetworkInfo`]: A struct representing a known network with its name, namespace, and reference
//! - [`KnownNetworkEip155`]: Trait for convenient access to EVM networks (eip155 namespace)
//! - [`KnownNetworkSolana`]: Trait for convenient access to Solana networks
//! - [`KNOWN_NETWORKS`]: A static array of all well-known networks
//! - [`chain_id_by_network_name`]: Lookup function to get ChainId by network name
//! - [`network_name_by_chain_id`]: Reverse lookup function to get network name by ChainId
//!
//! # Namespace-Specific Traits
//!
//! The module provides two namespace-specific traits for better organization and flexibility:
//!
//! ## KnownNetworkEip155
//! Provides convenient static methods for all EVM networks (eip155 namespace):
//! - Base, Base Sepolia, Skale Base Sepolia
//! - Polygon, Polygon Amoy
//! - Avalanche, Avalanche Fuji
//! - Sei, Sei Testnet
//! - Abstract, Abstract Testnet
//! - XDC, XRPL EVM, Peaq, IoTeX, Story, Educhain
//! - Celo, Celo Alfajores
//! - BSC, BSC Testnet
//! - Monad, Monad Testnet
//!
//! ## KnownNetworkSolana
//! Provides convenient static methods for Solana networks:
//! - Solana mainnet
//! - Solana devnet
//!
//! # Supported Networks
//!
//! The module supports 31 blockchain networks across two namespaces:
//! - **EVM Networks (23)**: All networks in the eip155 namespace
//! - **Solana Networks (2)**: Solana mainnet and devnet
//!
//! # Examples
//!
//! ```ignore
//! use x402_rs::chain::ChainId;
//! use x402_rs::known::{KnownNetworkEip155, KnownNetworkSolana, chain_id_by_network_name};
//!
//! // Using EVM network trait methods
//! let base = ChainId::base();
//! assert_eq!(base.namespace, "eip155");
//! assert_eq!(base.reference, "8453");
//!
//! // Using Solana network trait methods
//! let solana = ChainId::solana();
//! assert_eq!(solana.namespace, "solana");
//! assert_eq!(solana.reference, "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
//!
//! // Using lookup functions
//! let polygon = chain_id_by_network_name("polygon").unwrap();
//! assert_eq!(polygon.namespace, "eip155");
//! assert_eq!(polygon.reference, "137");
//!
//! // Reverse lookup
//! let chain_id = ChainId::new("solana", "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
//! let name = chain_id_by_network_name("solana").unwrap();
//! assert_eq!(name, "solana");
//! ```

use crate::chain::solana::{SolanaChainReference, SolanaTokenDeployment};
use crate::chain::{ChainId, solana};
use once_cell::sync::Lazy;
use solana_pubkey::pubkey;
use std::collections::HashMap;

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

/// Trait providing convenient methods to get instances for well-known EVM networks (eip155 namespace).
///
/// This trait can be implemented for any type to provide static methods that create
/// instances for well-known EVM blockchain networks. Each method returns `Self`, allowing
/// the trait to be used with different types that need per-network configuration.
///
/// # Use Cases
///
/// - **ChainId**: Get CAIP-2 chain identifiers for EVM networks
/// - **Token Deployments**: Get per-chain token addresses (e.g., USDC on different EVM chains)
/// - **Network Configuration**: Get network-specific configuration objects for EVM chains
/// - **Any Per-Network Data**: Any type that needs EVM network-specific instances
///
/// # Examples
///
/// ```ignore
/// use x402_rs::chain::ChainId;
/// use x402_rs::known::KnownNetworkEip155;
///
/// // Get Base mainnet chain ID
/// let base = ChainId::base();
/// assert_eq!(base.namespace, "eip155");
/// assert_eq!(base.reference, "8453");
///
/// // Get Polygon mainnet chain ID
/// let polygon = ChainId::polygon();
/// assert_eq!(polygon.namespace, "eip155");
/// assert_eq!(polygon.reference, "137");
///
/// // Can also be implemented for other types like token addresses
/// // let usdc_base = UsdcAddress::base();
/// // let usdc_polygon = UsdcAddress::polygon();
/// ```
#[allow(dead_code)]
pub trait KnownNetworkEip155<A> {
    /// Returns the instance for Base mainnet (eip155:8453)
    fn base() -> A;
    /// Returns the instance for Base Sepolia testnet (eip155:84532)
    fn base_sepolia() -> A;
    /// Returns the instance for Skale Base Sepolia (eip155:324705682)
    fn skale_base_sepolia() -> A;

    /// Returns the instance for Polygon mainnet (eip155:137)
    fn polygon() -> A;
    /// Returns the instance for Polygon Amoy testnet (eip155:80002)
    fn polygon_amoy() -> A;

    /// Returns the instance for Avalanche C-Chain mainnet (eip155:43114)
    fn avalanche() -> A;
    /// Returns the instance for Avalanche Fuji testnet (eip155:43113)
    fn avalanche_fuji() -> A;

    /// Returns the instance for Sei mainnet (eip155:1329)
    fn sei() -> A;
    /// Returns the instance for Sei testnet (eip155:1328)
    fn sei_testnet() -> A;

    /// Returns the instance for Abstract mainnet (eip155:2741)
    fn abstract_network() -> A;
    /// Returns the instance for Abstract testnet (eip155:11124)
    fn abstract_testnet() -> A;

    /// Returns the instance for XDC Network (eip155:50)
    fn xdc() -> A;

    /// Returns the instance for XRPL EVM (eip155:1440000)
    fn xrpl_evm() -> A;

    /// Returns the instance for Peaq (eip155:3338)
    fn peaq() -> A;

    /// Returns the instance for IoTeX (eip155:4689)
    fn iotex() -> A;

    /// Returns the instance for Story (eip155:1514)
    fn story() -> A;

    /// Returns the instance for Educhain (eip155:41923)
    fn educhain() -> A;

    /// Returns the instance for Celo mainnet (eip155:42220)
    fn celo() -> A;
    /// Returns the instance for Celo Alfajores testnet (eip155:44787)
    fn celo_alfajores() -> A;

    /// Returns the instance for Binance Smart Chain mainnet (eip155:56)
    fn bsc() -> A;
    /// Returns the instance for Binance Smart Chain testnet (eip155:97)
    fn bsc_testnet() -> A;

    /// Returns the instance for Monad (eip155:143)
    fn monad() -> A;
    /// Returns the instance for Monad testnet (eip155:10143)
    fn monad_testnet() -> A;
}

/// Trait providing convenient methods to get instances for well-known Solana networks.
///
/// This trait can be implemented for any type to provide static methods that create
/// instances for well-known Solana blockchain networks. Each method returns `Self`, allowing
/// the trait to be used with different types that need per-network configuration.
///
/// # Use Cases
///
/// - **ChainId**: Get CAIP-2 chain identifiers for Solana networks
/// - **Token Deployments**: Get per-chain token addresses (e.g., USDC on different Solana networks)
/// - **Network Configuration**: Get network-specific configuration objects for Solana chains
/// - **Any Per-Network Data**: Any type that needs Solana network-specific instances
///
/// # Examples
///
/// ```ignore
/// use x402_rs::chain::ChainId;
/// use x402_rs::known::KnownNetworkSolana;
///
/// // Get Solana mainnet chain ID
/// let solana = ChainId::solana();
/// assert_eq!(solana.namespace, "solana");
/// assert_eq!(solana.reference, "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
///
/// // Get Solana devnet chain ID
/// let devnet = ChainId::solana_devnet();
/// assert_eq!(devnet.namespace, "solana");
/// ```
#[allow(dead_code)]
pub trait KnownNetworkSolana<A> {
    /// Returns the instance for Solana mainnet (solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp)
    fn solana() -> A;
    /// Returns the instance for Solana devnet (solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1)
    fn solana_devnet() -> A;
}

/// Implementation of KnownNetworkEip155 for ChainId.
///
/// Provides convenient static methods to create ChainId instances for well-known
/// EVM blockchain networks. Each method returns a properly configured ChainId with the
/// "eip155" namespace and the correct chain reference.
///
/// This is one example of implementing the KnownNetworkEip155 trait. Other types
/// (such as token address types) can also implement this trait to provide
/// per-network instances with better developer experience.
impl KnownNetworkEip155<ChainId> for ChainId {
    fn base() -> ChainId {
        ChainId::new("eip155", "8453")
    }

    fn base_sepolia() -> ChainId {
        ChainId::new("eip155", "84532")
    }

    fn skale_base_sepolia() -> ChainId {
        ChainId::new("eip155", "324705682")
    }

    fn polygon() -> ChainId {
        ChainId::new("eip155", "137")
    }

    fn polygon_amoy() -> ChainId {
        ChainId::new("eip155", "80002")
    }

    fn avalanche() -> ChainId {
        ChainId::new("eip155", "43114")
    }

    fn avalanche_fuji() -> ChainId {
        ChainId::new("eip155", "43113")
    }

    fn sei() -> ChainId {
        ChainId::new("eip155", "1329")
    }

    fn sei_testnet() -> ChainId {
        ChainId::new("eip155", "1328")
    }

    fn abstract_network() -> ChainId {
        ChainId::new("eip155", "2741")
    }

    fn abstract_testnet() -> ChainId {
        ChainId::new("eip155", "11124")
    }

    fn xdc() -> ChainId {
        ChainId::new("eip155", "50")
    }

    fn xrpl_evm() -> ChainId {
        ChainId::new("eip155", "1440000")
    }

    fn peaq() -> ChainId {
        ChainId::new("eip155", "3338")
    }

    fn iotex() -> ChainId {
        ChainId::new("eip155", "4689")
    }

    fn story() -> ChainId {
        ChainId::new("eip155", "1514")
    }

    fn educhain() -> ChainId {
        ChainId::new("eip155", "41923")
    }

    fn celo() -> ChainId {
        ChainId::new("eip155", "42220")
    }

    fn celo_alfajores() -> ChainId {
        ChainId::new("eip155", "44787")
    }

    fn bsc() -> ChainId {
        ChainId::new("eip155", "56")
    }

    fn bsc_testnet() -> ChainId {
        ChainId::new("eip155", "97")
    }

    fn monad() -> ChainId {
        ChainId::new("eip155", "143")
    }

    fn monad_testnet() -> ChainId {
        ChainId::new("eip155", "10143")
    }
}

/// Implementation of KnownNetworkSolana for ChainId.
///
/// Provides convenient static methods to create ChainId instances for well-known
/// Solana blockchain networks. Each method returns a properly configured ChainId with the
/// "solana" namespace and the correct chain reference.
///
/// This is one example of implementing the KnownNetworkSolana trait. Other types
/// (such as token address types) can also implement this trait to provide
/// per-network instances with better developer experience.
impl KnownNetworkSolana<ChainId> for ChainId {
    fn solana() -> ChainId {
        solana::SolanaChainReference::solana().into()
    }

    fn solana_devnet() -> ChainId {
        solana::SolanaChainReference::solana_devnet().into()
    }
}

/// A static array of well-known blockchain networks.
///
/// This array contains a registry of well-known blockchain networks for improved
/// developer experience and x402 protocol v1 compatibility, organized by ecosystem
/// (EVM networks first, then Solana networks). Each entry includes the network's
/// human-readable name, CAIP-2 namespace, and chain reference.
///
/// The array is used to populate the lazy-initialized lookup hashmaps:
/// - [`NAME_TO_CHAIN_ID`] for name-based lookups
/// - [`CHAIN_ID_TO_NAME`] for ChainId-based lookups
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

/// Lazy-initialized hashmap for network name to ChainId lookups.
///
/// Maps human-readable network names (e.g., "base", "polygon", "solana") to their
/// corresponding [`ChainId`] instances. This hashmap is populated once on first access
/// from the [`KNOWN_NETWORKS`] array.
///
/// # Examples
///
/// ```ignore
/// use x402_rs::known::chain_id_by_network_name;
///
/// let base = chain_id_by_network_name("base").unwrap();
/// assert_eq!(base.namespace, "eip155");
/// assert_eq!(base.reference, "8453");
/// ```
static NAME_TO_CHAIN_ID: Lazy<HashMap<&'static str, ChainId>> = Lazy::new(|| {
    KNOWN_NETWORKS
        .iter()
        .map(|n| (n.name, n.chain_id()))
        .collect()
});

/// Lazy-initialized hashmap for ChainId to network name lookups.
///
/// Maps [`ChainId`] instances to their human-readable network names. This hashmap is
/// populated once on first access from the [`KNOWN_NETWORKS`] array. Useful for
/// reverse lookups when you have a ChainId and need to find its network name.
///
/// # Examples
///
/// ```ignore
/// use x402_rs::chain::ChainId;
/// use x402_rs::known::network_name_by_chain_id;
///
/// let chain_id = ChainId::new("eip155", "137");
/// let name = network_name_by_chain_id(&chain_id).unwrap();
/// assert_eq!(name, "polygon");
/// ```
static CHAIN_ID_TO_NAME: Lazy<HashMap<ChainId, &'static str>> = Lazy::new(|| {
    KNOWN_NETWORKS
        .iter()
        .map(|n| (n.chain_id(), n.name))
        .collect()
});

/// Retrieves a ChainId by its network name.
///
/// Performs a lookup in the [`NAME_TO_CHAIN_ID`] hashmap to find the ChainId
/// corresponding to the given network name. The lookup is case-sensitive.
///
/// # Arguments
///
/// * `name` - The human-readable network name (e.g., "base", "polygon-amoy", "solana")
///
/// # Returns
///
/// Returns `Some(&ChainId)` if the network name is found, or `None` if the name
/// is not in the known networks registry.
///
/// # Examples
///
/// ```ignore
/// use x402_rs::known::chain_id_by_network_name;
///
/// let base = chain_id_by_network_name("base").unwrap();
/// assert_eq!(base.namespace, "eip155");
/// assert_eq!(base.reference, "8453");
///
/// assert!(chain_id_by_network_name("unknown-network").is_none());
/// ```
pub fn chain_id_by_network_name(name: &str) -> Option<&ChainId> {
    NAME_TO_CHAIN_ID.get(name)
}

/// Retrieves a network name by its ChainId.
///
/// Performs a reverse lookup in the [`CHAIN_ID_TO_NAME`] hashmap to find the
/// human-readable network name corresponding to the given ChainId.
///
/// # Arguments
///
/// * `chain_id` - A reference to the ChainId to look up
///
/// # Returns
///
/// Returns `Some(&'static str)` containing the network name if the ChainId is found,
/// or `None` if the ChainId is not in the known networks registry.
///
/// # Examples
///
/// ```ignore
/// use x402_rs::chain::ChainId;
/// use x402_rs::known::network_name_by_chain_id;
///
/// let chain_id = ChainId::new("eip155", "8453");
/// let name = network_name_by_chain_id(&chain_id).unwrap();
/// assert_eq!(name, "base");
///
/// let unknown = ChainId::new("eip155", "999999");
/// assert!(network_name_by_chain_id(&unknown).is_none());
/// ```
pub fn network_name_by_chain_id(chain_id: &ChainId) -> Option<&'static str> {
    CHAIN_ID_TO_NAME.get(chain_id).copied()
}

pub struct USDC;

impl KnownNetworkSolana<SolanaTokenDeployment> for USDC {
    fn solana() -> SolanaTokenDeployment {
        let address = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
        SolanaTokenDeployment::new(SolanaChainReference::solana(), address.into(), 6)
    }

    fn solana_devnet() -> SolanaTokenDeployment {
        SolanaTokenDeployment::new(
            SolanaChainReference::solana_devnet(),
            pubkey!("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU").into(),
            6,
        )
    }
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
        let chain_id = chain_id_by_network_name("base").unwrap();
        assert_eq!(chain_id.namespace, "eip155");
        assert_eq!(chain_id.reference, "8453");

        let solana_chain_id = chain_id_by_network_name("solana").unwrap();
        assert_eq!(solana_chain_id.namespace, "solana");
        assert_eq!(
            solana_chain_id.reference,
            "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"
        );

        assert!(chain_id_by_network_name("unknown").is_none());
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
