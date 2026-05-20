//! Server-side price tag generation for V2 EIP-155 upto scheme.
//!
//! Servers MUST pass the facilitator EOA address so it can be bound into the
//! Permit2 witness: the canonical `x402UptoPermit2Proxy` enforces
//! `msg.sender == witness.facilitator` at settle time.

use alloy_primitives::{Address, U256};
use x402_types::chain::{ChainId, DeployedTokenAmount};
use x402_types::proto::v2;

use crate::V2Eip155Upto;
use crate::chain::{ChecksummedAddress, Eip155TokenDeployment};
use crate::v2_eip155_upto::types::{UptoExtra, UptoScheme};

impl V2Eip155Upto {
    /// Creates a V2 price tag for an upto payment on an EVM chain.
    ///
    /// # Parameters
    ///
    /// - `pay_to`: The recipient address
    /// - `asset`: The token deployment and maximum amount authorized
    /// - `facilitator_address`: The facilitator EOA that will execute on-chain settlement;
    ///   bound into the Permit2 witness and enforced by the proxy as `msg.sender`.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use alloy_primitives::address;
    /// use x402_chain_eip155::{V2Eip155Upto, KnownNetworkEip155};
    /// use x402_types::networks::USDC;
    ///
    /// let usdc = USDC::base();
    /// let price_tag = V2Eip155Upto::price_tag(
    ///     "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb",
    ///     usdc.amount(5_000_000u64), // up to 5 USDC
    ///     address!("0x0000000000000000000000000000000000000000"),
    /// );
    /// ```
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn price_tag<A: Into<ChecksummedAddress>>(
        pay_to: A,
        asset: DeployedTokenAmount<U256, Eip155TokenDeployment>,
        facilitator_address: Address,
    ) -> v2::PriceTag {
        let chain_id: ChainId = asset.token.chain_reference.into();
        let extra = serde_json::to_value(UptoExtra { facilitator_address }).ok();
        let requirements = v2::PaymentRequirements {
            scheme: UptoScheme.to_string(),
            pay_to: pay_to.into().to_string(),
            asset: asset.token.address.to_string(),
            network: chain_id,
            amount: asset.amount.to_string(),
            max_timeout_seconds: 300,
            extra,
        };
        v2::PriceTag {
            requirements,
            enricher: None,
        }
    }
}
