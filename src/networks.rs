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

// FIXME Find #[cfg(feature = "aptos")] and clean it

#[cfg(test)]
mod tests {
    use super::*;
    use x402_types::networks::{chain_id_by_network_name, network_name_by_chain_id};

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
