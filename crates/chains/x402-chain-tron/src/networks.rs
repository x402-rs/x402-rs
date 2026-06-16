//! Known TRON networks and token deployments.

use x402_types::chain::ChainId;

use crate::chain::{TronNetwork, TronTokenDeployment, TronTransferMethod};

/// Marker struct for USDT (Tether) token deployment implementations on TRON.
#[allow(dead_code, clippy::upper_case_acronyms)]
pub struct USDT;

/// Trait providing convenient methods to get instances for well-known TRON networks.
///
/// Implement this trait for a type `A` to expose `mainnet()` and `nile()` constructors
/// that produce network-specific instances — just as `KnownNetworkEip155` does for EVM.
///
/// # Examples
///
/// ```ignore
/// use x402_types::chain::ChainId;
/// use x402_chain_tron::KnownNetworkTron;
///
/// let mainnet = ChainId::mainnet();
/// assert_eq!(mainnet.to_string(), "tron:mainnet");
///
/// let nile = ChainId::nile();
/// assert_eq!(nile.to_string(), "tron:nile");
/// ```
#[allow(dead_code)]
pub trait KnownNetworkTron<A> {
    /// Returns the instance for TRON mainnet (`tron:mainnet`).
    fn mainnet() -> A;
    /// Returns the instance for TRON Nile testnet (`tron:nile`).
    fn nile() -> A;
}

// ── ChainId ─────────────────────────────────────────────────────────────────

impl KnownNetworkTron<ChainId> for ChainId {
    fn mainnet() -> ChainId {
        TronNetwork::Mainnet.chain_id()
    }

    fn nile() -> ChainId {
        TronNetwork::Nile.chain_id()
    }
}

// ── USDT ────────────────────────────────────────────────────────────────────

impl KnownNetworkTron<TronTokenDeployment> for USDT {
    fn mainnet() -> TronTokenDeployment {
        TronTokenDeployment {
            network: TronNetwork::Mainnet,
            address: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".to_string(),
            decimals: 6,
            transfer_method: TronTransferMethod::Eip3009 {
                name: "Tether USD".to_string(),
                version: "1".to_string(),
            },
        }
    }

    fn nile() -> TronTokenDeployment {
        TronTokenDeployment {
            network: TronNetwork::Nile,
            address: "TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf".to_string(),
            decimals: 6,
            transfer_method: TronTransferMethod::Eip3009 {
                name: "Tether USD".to_string(),
                version: "1".to_string(),
            },
        }
    }
}

/// Permit2 proxy contract addresses on TRON networks.
pub mod permit2 {
    /// Permit2 proxy on TRON mainnet.
    pub const MAINNET_BASE58: &str = "TTJxU3P8rHycAyFY4kVtGNfmnMH4ezcuM9";
    /// Permit2 proxy on TRON Nile testnet.
    pub const NILE_BASE58: &str = "TCJjTtzwRJYPapGTdyJdKcr7MqkngRRWQx";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_id_mainnet() {
        let id = ChainId::mainnet();
        assert_eq!(id.namespace, "tron");
        assert_eq!(id.reference, "mainnet");
    }

    #[test]
    fn chain_id_nile() {
        let id = ChainId::nile();
        assert_eq!(id.namespace, "tron");
        assert_eq!(id.reference, "nile");
    }

    #[test]
    fn usdt_mainnet() {
        let t = USDT::mainnet();
        assert_eq!(t.network, TronNetwork::Mainnet);
        assert_eq!(t.address, "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t");
        assert_eq!(t.decimals, 6);
    }
}
