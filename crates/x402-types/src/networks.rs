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
//! - Base, Base Sepolia
//! - Polygon, Polygon Amoy
//! - Avalanche, Avalanche Fuji
//! - Sei, Sei Testnet
//! - XDC, XRPL EVM, Peaq, IoTeX
//!
//! ## KnownNetworkSolana
//! Provides convenient static methods for Solana networks:
//! - Solana mainnet
//! - Solana devnet
//!
//! # Supported Networks
//!
//! The module supports 14 blockchain networks across two namespaces:
//! - **EVM Networks (12)**: All networks in the eip155 namespace
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

use std::collections::HashMap;
use std::sync::LazyLock;

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
pub static KNOWN_NETWORKS: &[NetworkInfo] = &[
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
    // Celo Networks
    NetworkInfo {
        name: "celo",
        namespace: "eip155",
        reference: "42220",
    },
    NetworkInfo {
        name: "celo-sepolia",
        namespace: "eip155",
        reference: "11142220",
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
static NAME_TO_CHAIN_ID: LazyLock<HashMap<&'static str, ChainId>> = LazyLock::new(|| {
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
static CHAIN_ID_TO_NAME: LazyLock<HashMap<ChainId, &'static str>> = LazyLock::new(|| {
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

#[allow(dead_code, clippy::upper_case_acronyms)] // Public for consumption by downstream crates.
pub struct USDC;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_id_from_network_name() {
        let base = chain_id_by_network_name("base").unwrap();
        assert_eq!(base.namespace, "eip155");
        assert_eq!(base.reference, "8453");

        let base_sepolia = chain_id_by_network_name("base-sepolia").unwrap();
        assert_eq!(base_sepolia.namespace, "eip155");
        assert_eq!(base_sepolia.reference, "84532");

        let polygon = chain_id_by_network_name("polygon").unwrap();
        assert_eq!(polygon.namespace, "eip155");
        assert_eq!(polygon.reference, "137");

        let celo = chain_id_by_network_name("celo").unwrap();
        assert_eq!(celo.namespace, "eip155");
        assert_eq!(celo.reference, "42220");

        let solana = chain_id_by_network_name("solana").unwrap();
        assert_eq!(solana.namespace, "solana");
        assert_eq!(solana.reference, "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");

        assert!(chain_id_by_network_name("unknown").is_none());
    }

    #[test]
    fn test_network_name_by_chain_id() {
        let chain_id = ChainId::new("eip155", "8453");
        let network_name = network_name_by_chain_id(&chain_id).unwrap();
        assert_eq!(network_name, "base");

        let celo_chain_id = ChainId::new("eip155", "42220");
        let network_name = network_name_by_chain_id(&celo_chain_id).unwrap();
        assert_eq!(network_name, "celo");

        let celo_sepolia_chain_id = ChainId::new("eip155", "11142220");
        let network_name = network_name_by_chain_id(&celo_sepolia_chain_id).unwrap();
        assert_eq!(network_name, "celo-sepolia");

        let solana_chain_id = ChainId::new("solana", "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
        let network_name = network_name_by_chain_id(&solana_chain_id).unwrap();
        assert_eq!(network_name, "solana");

        let unknown_chain_id = ChainId::new("eip155", "999999");
        assert!(network_name_by_chain_id(&unknown_chain_id).is_none());
    }

    #[test]
    fn test_chain_id_as_network_name() {
        let chain_id = ChainId::new("eip155", "8453");
        assert_eq!(chain_id.as_network_name(), Some("base"));

        let celo_chain_id = ChainId::new("eip155", "42220");
        assert_eq!(celo_chain_id.as_network_name(), Some("celo"));

        let solana_chain_id = ChainId::new("solana", "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
        assert_eq!(solana_chain_id.as_network_name(), Some("solana"));

        let unknown_chain_id = ChainId::new("eip155", "999999");
        assert!(unknown_chain_id.as_network_name().is_none());
    }
}
