//! Known TRON networks and token deployments.
//!
//! Chain IDs follow the CAIP-2 format for the `tron` namespace — hex-encoded
//! last-4-bytes of the genesis block hash, prefixed with `0x`.
//! See <https://github.com/ChainAgnostic/namespaces/pull/170>.

use x402_types::chain::ChainId;
use x402_types::networks::USDC;

use crate::chain::{TronChainReference, TronTokenDeployment, TronTransferMethod};

/// Marker struct for USDT (Tether) token deployment implementations on TRON.
#[allow(dead_code, clippy::upper_case_acronyms)]
pub struct USDT;

/// Trait providing convenient methods to get instances for well-known TRON networks.
///
/// Implement this for a type `A` to expose `mainnet()`, `shasta()`, and `nile()`
/// constructors — mirroring the `KnownNetworkEip155` / `KnownNetworkSolana` pattern.
///
/// | Network | CAIP-2            | Chain ID   |
/// |---------|-------------------|------------|
/// | Mainnet | `tron:0x2b6653dc` | 728126428  |
/// | Shasta  | `tron:0xcd8690dc` | 3448148188 |
/// | Nile    | `tron:0x94a9059e` | 2494104990 |
#[allow(dead_code)]
pub trait KnownNetworkTron<A> {
    /// Returns the instance for TRON mainnet (`tron:0x2b6653dc`).
    fn mainnet() -> A;
    /// Returns the instance for TRON Shasta testnet (`tron:0xcd8690dc`).
    fn shasta() -> A;
    /// Returns the instance for TRON Nile testnet (`tron:0x94a9059e`).
    fn nile() -> A;
}

// ── TronChainReference ───────────────────────────────────────────────────────

impl KnownNetworkTron<TronChainReference> for TronChainReference {
    fn mainnet() -> TronChainReference {
        TronChainReference::new(0x2b6653dc)
    }

    fn shasta() -> TronChainReference {
        TronChainReference::new(0xcd8690dc)
    }

    fn nile() -> TronChainReference {
        TronChainReference::new(0x94a9059e)
    }
}

// ── ChainId ──────────────────────────────────────────────────────────────────

impl KnownNetworkTron<ChainId> for ChainId {
    fn mainnet() -> ChainId {
        TronChainReference::mainnet().into()
    }

    fn shasta() -> ChainId {
        TronChainReference::shasta().into()
    }

    fn nile() -> ChainId {
        TronChainReference::nile().into()
    }
}

// ── USDC ─────────────────────────────────────────────────────────────────────

impl KnownNetworkTron<TronTokenDeployment> for USDC {
    fn mainnet() -> TronTokenDeployment {
        TronTokenDeployment {
            chain_reference: TronChainReference::mainnet(),
            address: "TEkxiTehnzSmSe2XqrBj4w32RUN966rdz8".try_into().unwrap(),
            decimals: 6,
            transfer_method: TronTransferMethod::Eip3009 {
                name: "USD Coin".to_string(),
                version: "2".to_string(),
            },
        }
    }

    fn shasta() -> TronTokenDeployment {
        TronTokenDeployment {
            chain_reference: TronChainReference::shasta(),
            address: "TXYZopYRdj2D9XRtbG411XZZ3kM5VkAeBf".try_into().unwrap(),
            decimals: 6,
            transfer_method: TronTransferMethod::Eip3009 {
                name: "USD Coin".to_string(),
                version: "2".to_string(),
            },
        }
    }

    fn nile() -> TronTokenDeployment {
        TronTokenDeployment {
            chain_reference: TronChainReference::nile(),
            address: "TF17BgPaZYbz8oxbjhriubPDsA7ArKoLX3".try_into().unwrap(),
            decimals: 6,
            transfer_method: TronTransferMethod::Eip3009 {
                name: "USD Coin".to_string(),
                version: "2".to_string(),
            },
        }
    }
}

// ── USDT ─────────────────────────────────────────────────────────────────────

impl KnownNetworkTron<TronTokenDeployment> for USDT {
    fn mainnet() -> TronTokenDeployment {
        TronTokenDeployment {
            chain_reference: TronChainReference::mainnet(),
            address: "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t".try_into().unwrap(),
            decimals: 6,
            transfer_method: TronTransferMethod::Eip3009 {
                name: "Tether USD".to_string(),
                version: "1".to_string(),
            },
        }
    }

    fn shasta() -> TronTokenDeployment {
        TronTokenDeployment {
            chain_reference: TronChainReference::shasta(),
            address: "TQQg4EL8o1BSeKJY4MJ8TB8XK7xufxFBvK".try_into().unwrap(),
            decimals: 6,
            transfer_method: TronTransferMethod::Eip3009 {
                name: "Tether USD".to_string(),
                version: "1".to_string(),
            },
        }
    }

    fn nile() -> TronTokenDeployment {
        TronTokenDeployment {
            chain_reference: TronChainReference::nile(),
            address: "TXLAQ63Xg1NAzckPwKHvzw7CSEmLMEqcdj".try_into().unwrap(),
            decimals: 6,
            transfer_method: TronTransferMethod::Eip3009 {
                name: "Tether USD".to_string(),
                version: "1".to_string(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chain_reference_display() {
        assert_eq!(TronChainReference::mainnet().to_string(), "0x2b6653dc");
        assert_eq!(TronChainReference::shasta().to_string(), "0xcd8690dc");
        assert_eq!(TronChainReference::nile().to_string(), "0x94a9059e");
    }

    #[test]
    fn chain_id_format() {
        assert_eq!(ChainId::mainnet().to_string(), "tron:0x2b6653dc");
        assert_eq!(ChainId::shasta().to_string(), "tron:0xcd8690dc");
        assert_eq!(ChainId::nile().to_string(), "tron:0x94a9059e");
    }

    #[test]
    fn chain_reference_round_trips() {
        for r in [
            TronChainReference::mainnet(),
            TronChainReference::shasta(),
            TronChainReference::nile(),
        ] {
            let chain_id = ChainId::from(r.clone());
            let parsed = TronChainReference::try_from(chain_id).unwrap();
            assert_eq!(parsed, r);
        }
    }

    #[test]
    fn eip712_chain_ids() {
        assert_eq!(TronChainReference::mainnet().inner(), 728126428);
        assert_eq!(TronChainReference::shasta().inner(), 3448148188);
        assert_eq!(TronChainReference::nile().inner(), 2494104990);
    }

    #[test]
    fn permit2_proxies() {
        assert!(TronChainReference::mainnet().permit2_proxy().is_some());
        assert!(TronChainReference::shasta().permit2_proxy().is_some());
        assert!(TronChainReference::nile().permit2_proxy().is_none());
    }

    #[test]
    fn usdt_mainnet() {
        let t = USDT::mainnet();
        assert_eq!(t.chain_reference, TronChainReference::mainnet());
        assert_eq!(t.address, "TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t");
        assert_eq!(t.decimals, 6);
    }

    #[test]
    fn usdc_mainnet() {
        let u = USDC::mainnet();
        assert_eq!(u.chain_reference, TronChainReference::mainnet());
        assert_eq!(u.address, "TEkxiTehnzSmSe2XqrBj4w32RUN966rdz8");
        assert_eq!(u.decimals, 6);
    }
}
