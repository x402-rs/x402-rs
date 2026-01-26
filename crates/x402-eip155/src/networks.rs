use x402_types::chain::ChainId;

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

    /// Returns the instance for XDC Network (eip155:50)
    fn xdc() -> A;

    /// Returns the instance for XRPL EVM (eip155:1440000)
    fn xrpl_evm() -> A;

    /// Returns the instance for Peaq (eip155:3338)
    fn peaq() -> A;

    /// Returns the instance for IoTeX (eip155:4689)
    fn iotex() -> A;

    /// Returns the instance for Celo mainnet (eip155:42220)
    fn celo() -> A;

    /// Returns the instance for Celo testnet (eip155:11142220)
    fn celo_sepolia() -> A;
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

    fn celo() -> ChainId {
        ChainId::new("eip155", "42220")
    }

    fn celo_sepolia() -> ChainId {
        ChainId::new("eip155", "11142220")
    }
}
