use x402_types::chain::ChainId;
use x402_types::networks::USDC;

use crate::chain::{Eip155ChainReference, Eip155TokenDeployment, TokenDeploymentEip712};

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

impl KnownNetworkEip155<Eip155TokenDeployment> for USDC {
    fn base() -> Eip155TokenDeployment {
        Eip155TokenDeployment {
            chain_reference: Eip155ChainReference::new(8453),
            address: alloy_primitives::address!("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"),
            decimals: 6,
            eip712: Some(TokenDeploymentEip712 {
                name: "USD Coin".into(),
                version: "2".into(),
            }),
        }
    }

    fn base_sepolia() -> Eip155TokenDeployment {
        Eip155TokenDeployment {
            chain_reference: Eip155ChainReference::new(84532),
            address: alloy_primitives::address!("0x036CbD53842c5426634e7929541eC2318f3dCF7e"),
            decimals: 6,
            eip712: Some(TokenDeploymentEip712 {
                name: "USDC".into(),
                version: "2".into(),
            }),
        }
    }

    fn polygon() -> Eip155TokenDeployment {
        Eip155TokenDeployment {
            chain_reference: Eip155ChainReference::new(137),
            address: alloy_primitives::address!("0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359"),
            decimals: 6,
            eip712: Some(TokenDeploymentEip712 {
                name: "USDC".into(),
                version: "2".into(),
            }),
        }
    }

    fn polygon_amoy() -> Eip155TokenDeployment {
        Eip155TokenDeployment {
            chain_reference: Eip155ChainReference::new(80002),
            address: alloy_primitives::address!("0x41E94Eb019C0762f9Bfcf9Fb1E58725BfB0e7582"),
            decimals: 6,
            eip712: Some(TokenDeploymentEip712 {
                name: "USDC".into(),
                version: "2".into(),
            }),
        }
    }

    fn avalanche() -> Eip155TokenDeployment {
        Eip155TokenDeployment {
            chain_reference: Eip155ChainReference::new(43114),
            address: alloy_primitives::address!("0xB97EF9Ef8734C71904D8002F8b6Bc66Dd9c48a6E"),
            decimals: 6,
            eip712: Some(TokenDeploymentEip712 {
                name: "USD Coin".into(),
                version: "2".into(),
            }),
        }
    }

    fn avalanche_fuji() -> Eip155TokenDeployment {
        Eip155TokenDeployment {
            chain_reference: Eip155ChainReference::new(43113),
            address: alloy_primitives::address!("0x5425890298aed601595a70AB815c96711a31Bc65"),
            decimals: 6,
            eip712: Some(TokenDeploymentEip712 {
                name: "USD Coin".into(),
                version: "2".into(),
            }),
        }
    }

    fn sei() -> Eip155TokenDeployment {
        Eip155TokenDeployment {
            chain_reference: Eip155ChainReference::new(1329),
            address: alloy_primitives::address!("0xe15fC38F6D8c56aF07bbCBe3BAf5708A2Bf42392"),
            decimals: 6,
            eip712: Some(TokenDeploymentEip712 {
                name: "USDC".into(),
                version: "2".into(),
            }),
        }
    }

    fn sei_testnet() -> Eip155TokenDeployment {
        Eip155TokenDeployment {
            chain_reference: Eip155ChainReference::new(1328),
            address: alloy_primitives::address!("0x4fCF1784B31630811181f670Aea7A7bEF803eaED"),
            decimals: 6,
            eip712: Some(TokenDeploymentEip712 {
                name: "USDC".into(),
                version: "2".into(),
            }),
        }
    }

    fn xdc() -> Eip155TokenDeployment {
        Eip155TokenDeployment {
            chain_reference: Eip155ChainReference::new(50),
            address: alloy_primitives::address!("0xfA2958CB79b0491CC627c1557F441eF849Ca8eb1"),
            decimals: 6,
            eip712: Some(TokenDeploymentEip712 {
                name: "USDC".into(),
                version: "2".into(),
            }),
        }
    }

    fn xrpl_evm() -> Eip155TokenDeployment {
        Eip155TokenDeployment {
            chain_reference: Eip155ChainReference::new(1440000),
            address: alloy_primitives::address!("0xDaF4556169c4F3f2231d8ab7BC8772Ddb7D4c84C"),
            decimals: 6,
            eip712: None,
        }
    }

    fn peaq() -> Eip155TokenDeployment {
        Eip155TokenDeployment {
            chain_reference: Eip155ChainReference::new(3338),
            address: alloy_primitives::address!("0xbbA60da06c2c5424f03f7434542280FCAd453d10"),
            decimals: 6,
            eip712: Some(TokenDeploymentEip712 {
                name: "USDC".into(),
                version: "2".into(),
            }),
        }
    }

    fn iotex() -> Eip155TokenDeployment {
        Eip155TokenDeployment {
            chain_reference: Eip155ChainReference::new(4689),
            address: alloy_primitives::address!("0xcdf79194c6c285077a58da47641d4dbe51f63542"),
            decimals: 6,
            eip712: Some(TokenDeploymentEip712 {
                name: "Bridged USDC".into(),
                version: "2".into(),
            }),
        }
    }

    fn celo() -> Eip155TokenDeployment {
        Eip155TokenDeployment {
            chain_reference: Eip155ChainReference::new(42220),
            address: alloy_primitives::address!("0xcebA9300f2b948710d2653dD7B07f33A8B32118C"),
            decimals: 6,
            eip712: Some(TokenDeploymentEip712 {
                name: "USDC".into(),
                version: "2".into(),
            }),
        }
    }

    fn celo_sepolia() -> Eip155TokenDeployment {
        Eip155TokenDeployment {
            chain_reference: Eip155ChainReference::new(11142220),
            address: alloy_primitives::address!("0x01C5C0122039549AD1493B8220cABEdD739BC44E"),
            decimals: 6,
            eip712: Some(TokenDeploymentEip712 {
                name: "USDC".into(),
                version: "2".into(),
            }),
        }
    }
}
