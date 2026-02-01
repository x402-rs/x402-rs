//! Known blockchain networks and CAIP-2 chain ID management.
//!
//! This module provides a comprehensive registry of well-known blockchain networks with their
//! corresponding CAIP-2 (Chain Agnostic Improvement Proposal 2) chain identifiers. It is designed
//! to improve developer experience (DX) when working with the x402 protocol, which operates on
//! CAIP-2 chain IDs.
//!
//! # x402 v1 Protocol Relevance
//!
//! **This module is primarily relevant for x402 v1 protocol compatibility.** The registry of
//! known networks represents the set of blockchain networks that were supported in x402 v1.
//! For x402 v2 and beyond, the protocol is designed to work with any CAIP-2 chain ID without
//! requiring a predefined registry.
//!
//! Despite being v1-focused, this module continues to provide value for improved developer
//! experience by offering convenient methods to work with well-known networks without manually
//! constructing CAIP-2 identifiers.
//!
//! # Purpose
//!
//! This module serves two main purposes:
//! 1. **x402 v1 Protocol Compatibility**: Maintains support for networks that were known in v1
//! 2. **Better Developer Experience**: Provides convenient methods to work with well-known networks
//!    without manually constructing CAIP-2 identifiers
//!
//! # Usage Across the Codebase
//!
//! This module is used in several ways throughout the x402 ecosystem:
//!
//! - **ChainId Methods**: The [`ChainId::from_network_name()`](crate::chain::ChainId::from_network_name)
//!   and [`ChainId::as_network_name()`](crate::chain::ChainId::as_network_name) methods use this
//!   module for convenient network name lookups
//! - **Chain-Specific Traits**: Chain-specific crates (e.g., `x402-chain-eip155`, `x402-chain-solana`)
//!   implement namespace-specific traits like [`KnownNetworkEip155`] and [`KnownNetworkSolana`]
//!   for type-safe network access
//! - **Token Deployments**: The [`USDC`] marker struct is used by chain-specific crates to provide
//!   per-network token deployment information (e.g., USDC addresses on different chains)
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
//! - [`USDC`]: Marker struct used for token deployment implementations
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
//! - Celo, Celo Sepolia
//!
//! ## KnownNetworkSolana
//! Provides convenient static methods for Solana networks:
//! - Solana mainnet
//! - Solana devnet
//!
//! # Supported Networks
//!
//! The module supports 16 blockchain networks across two namespaces:
//! - **EVM Networks (14)**: All networks in the eip155 namespace
//! - **Solana Networks (2)**: Solana mainnet and devnet
//!
//! # Examples
//!
//! ```
//! use x402_types::chain::ChainId;
//! use x402_types::networks::chain_id_by_network_name;
//!
//! // Using lookup functions
//! let polygon = chain_id_by_network_name("polygon").unwrap();
//! assert_eq!(polygon.namespace, "eip155");
//! assert_eq!(polygon.reference, "137");
//!
//! // Using ChainId::from_network_name
//! let base = ChainId::from_network_name("base").unwrap();
//! assert_eq!(base.namespace, "eip155");
//! assert_eq!(base.reference, "8453");
//!
//! // Reverse lookup
//! let chain_id = ChainId::new("solana", "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
//! assert_eq!(chain_id.as_network_name(), Some("solana"));
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
/// # x402 v1 Protocol Relevance
///
/// This registry represents the set of blockchain networks that were known and supported
/// in x402 v1. For x402 v2 and beyond, the protocol is designed to work with any CAIP-2
/// chain ID without requiring a predefined registry.
///
/// The array is used to populate the lazy-initialized lookup hashmaps:
/// - [`NAME_TO_CHAIN_ID`] for name-based lookups
/// - [`CHAIN_ID_TO_NAME`] for ChainId-based lookups
///
/// # Developer Experience Benefits
///
/// Despite being v1-focused, this registry continues to provide value by:
/// - Enabling convenient network name lookups via [`ChainId::from_network_name()`](crate::chain::ChainId::from_network_name)
/// - Providing human-readable network names via [`ChainId::as_network_name()`](crate::chain::ChainId::as_network_name)
/// - Serving as a reference for commonly used blockchain networks
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
/// # x402 v1 Protocol Relevance
///
/// This hashmap provides the network name lookup functionality that was used in x402 v1.
/// For x402 v2 and beyond, the protocol is designed to work with any CAIP-2 chain ID
/// without requiring a predefined registry.
///
/// # Developer Experience Benefits
///
/// Despite being v1-focused, this hashmap continues to provide value by enabling
/// convenient network name lookups via [`ChainId::from_network_name()`](crate::chain::ChainId::from_network_name).
///
/// # Examples
///
/// ```
/// use x402_types::networks::chain_id_by_network_name;
///
/// let base = chain_id_by_network_name("base").unwrap();
/// assert_eq!(base.namespace, "eip155");
/// assert_eq!(base.reference, "8453");
/// ```
pub static NAME_TO_CHAIN_ID: LazyLock<HashMap<&'static str, ChainId>> = LazyLock::new(|| {
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
/// # x402 v1 Protocol Relevance
///
/// This hashmap provides the reverse lookup functionality that was used in x402 v1.
/// For x402 v2 and beyond, the protocol is designed to work with any CAIP-2 chain ID
/// without requiring a predefined registry.
///
/// # Developer Experience Benefits
///
/// Despite being v1-focused, this hashmap continues to provide value by enabling
/// human-readable network name lookups via [`ChainId::as_network_name()`](crate::chain::ChainId::as_network_name).
///
/// # Examples
///
/// ```
/// use x402_types::chain::ChainId;
/// use x402_types::networks::network_name_by_chain_id;
///
/// let chain_id = ChainId::new("eip155", "137");
/// let name = network_name_by_chain_id(&chain_id).unwrap();
/// assert_eq!(name, "polygon");
/// ```
pub static CHAIN_ID_TO_NAME: LazyLock<HashMap<ChainId, &'static str>> = LazyLock::new(|| {
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
/// # x402 v1 Protocol Relevance
///
/// This function provides the network name lookup functionality that was used in x402 v1.
/// For x402 v2 and beyond, the protocol is designed to work with any CAIP-2 chain ID
/// without requiring a predefined registry.
///
/// # Developer Experience Benefits
///
/// Despite being v1-focused, this function continues to provide value by enabling
/// convenient network name lookups. It is used by [`ChainId::from_network_name()`](crate::chain::ChainId::from_network_name)
/// to provide a developer-friendly API for creating ChainId instances.
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
/// ```
/// use x402_types::networks::chain_id_by_network_name;
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
/// # x402 v1 Protocol Relevance
///
/// This function provides the reverse lookup functionality that was used in x402 v1.
/// For x402 v2 and beyond, the protocol is designed to work with any CAIP-2 chain ID
/// without requiring a predefined registry.
///
/// # Developer Experience Benefits
///
/// Despite being v1-focused, this function continues to provide value by enabling
/// human-readable network name lookups. It is used by [`ChainId::as_network_name()`](crate::chain::ChainId::as_network_name)
/// to provide a developer-friendly API for displaying network names.
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
/// ```
/// use x402_types::chain::ChainId;
/// use x402_types::networks::network_name_by_chain_id;
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

/// Marker struct for USDC token deployment implementations.
///
/// This struct is used as a type parameter for chain-specific traits (e.g., `KnownNetworkEip155`,
/// `KnownNetworkSolana`) to provide per-network USDC token deployment information.
///
/// # Usage
///
/// Chain-specific crates implement traits for this marker struct to provide USDC token
/// deployments on different networks. For example:
///
/// - `x402-chain-eip155` implements `KnownNetworkEip155<Eip155TokenDeployment>` for `USDC`
/// - `x402-chain-solana` implements `KnownNetworkSolana<SolanaTokenDeployment>` for `USDC`
/// - `x402-chain-aptos` implements `KnownNetworkAptos<AptosTokenDeployment>` for `USDC`
///
/// # Example
///
/// ```ignore
/// use x402_chain_eip155::{KnownNetworkEip155, Eip155TokenDeployment};
/// use x402_types::networks::USDC;
///
/// // Get USDC deployment on Base mainnet
/// let usdc_base: Eip155TokenDeployment = USDC::base();
/// assert_eq!(usdc_base.chain_reference.value(), 8453);
/// ```
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
