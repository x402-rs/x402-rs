use x402_types::chain::ChainId;
use x402_types::networks::USDC;

use crate::chain::{Address, AptosChainReference, AptosTokenDeployment};

/// Trait providing convenient methods to get instances for well-known Aptos networks.
///
/// This trait can be implemented for any type to provide static methods that create
/// instances for well-known Aptos blockchain networks. Each method returns `Self`, allowing
/// the trait to be used with different types that need per-network configuration.
///
/// # Use Cases
///
/// - **ChainId**: Get CAIP-2 chain identifiers for Aptos networks
/// - **Token Deployments**: Get per-chain token addresses (e.g., USDC on different Aptos networks)
/// - **Network Configuration**: Get network-specific configuration objects for Aptos chains
/// - **Any Per-Network Data**: Any type that needs Aptos network-specific instances
///
/// # Examples
///
/// ```ignore
/// use x402_rs::chain::ChainId;
/// use x402_rs::known::KnownNetworkAptos;
///
/// // Get Aptos mainnet chain ID
/// let aptos = ChainId::aptos();
/// assert_eq!(aptos.namespace, "aptos");
/// assert_eq!(aptos.reference, "1");
///
/// // Get Aptos testnet chain ID
/// let testnet = ChainId::aptos_testnet();
/// assert_eq!(testnet.namespace, "aptos");
/// assert_eq!(testnet.reference, "2");
/// ```
#[allow(dead_code)]
pub trait KnownNetworkAptos<A> {
    /// Returns the instance for Aptos mainnet (aptos:1)
    fn aptos() -> A;
    /// Returns the instance for Aptos testnet (aptos:2)
    fn aptos_testnet() -> A;
}

/// Implementation of KnownNetworkAptos for ChainId.
///
/// Provides convenient static methods to create ChainId instances for well-known
/// Aptos blockchain networks. Each method returns a properly configured ChainId with the
/// "aptos" namespace and the correct chain reference.
///
/// This is one example of implementing the KnownNetworkAptos trait. Other types
/// (such as token address types) can also implement this trait to provide
/// per-network instances with better developer experience.
impl KnownNetworkAptos<ChainId> for ChainId {
    fn aptos() -> ChainId {
        AptosChainReference::aptos().into()
    }

    fn aptos_testnet() -> ChainId {
        AptosChainReference::aptos_testnet().into()
    }
}

impl KnownNetworkAptos<AptosTokenDeployment> for USDC {
    fn aptos() -> AptosTokenDeployment {
        // USDC on Aptos mainnet (fungible asset metadata address)
        let address: Address = "0xbae207659db88bea0cbead6da0ed00aac12edcdda169e591cd41c94180b46f3b"
            .parse()
            .expect("Invalid USDC address");
        AptosTokenDeployment::new(AptosChainReference::aptos(), address, 6)
    }

    fn aptos_testnet() -> AptosTokenDeployment {
        // USDC on Aptos testnet (this is a placeholder address, actual testnet USDC may differ)
        let address: Address = "0xbae207659db88bea0cbead6da0ed00aac12edcdda169e591cd41c94180b46f3b"
            .parse()
            .expect("Invalid USDC address");
        AptosTokenDeployment::new(AptosChainReference::aptos_testnet(), address, 6)
    }
}
