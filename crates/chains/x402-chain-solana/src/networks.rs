use solana_pubkey::pubkey;
use x402_types::chain::ChainId;
use x402_types::networks::USDC;

use crate::chain::{SolanaChainReference, SolanaTokenDeployment};

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
        SolanaChainReference::solana().into()
    }

    fn solana_devnet() -> ChainId {
        SolanaChainReference::solana_devnet().into()
    }
}

impl KnownNetworkSolana<SolanaTokenDeployment> for USDC {
    fn solana() -> SolanaTokenDeployment {
        let address = pubkey!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
        SolanaTokenDeployment::new(SolanaChainReference::solana(), address.into(), 6)
    }

    fn solana_devnet() -> SolanaTokenDeployment {
        let address = pubkey!("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZJDncDU");
        SolanaTokenDeployment::new(SolanaChainReference::solana_devnet(), address.into(), 6)
    }
}
