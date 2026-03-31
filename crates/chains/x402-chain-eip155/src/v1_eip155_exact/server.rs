//! Server-side price tag generation for V1 EIP-155 exact scheme.
//!
//! This module provides functionality for servers to create price tags
//! that clients can use to generate payment authorizations.

use alloy_primitives::U256;
use x402_types::chain::{ChainId, DeployedTokenAmount};
use x402_types::proto::v1;

use crate::V1Eip155Exact;
use crate::chain::{ChecksummedAddress, Eip155TokenDeployment};
use crate::v1_eip155_exact::ExactScheme;

impl V1Eip155Exact {
    /// Creates a V1 price tag for an ERC-3009 payment on an EVM chain.
    ///
    /// This function generates a price tag that specifies the payment requirements
    /// for a resource. The price tag includes the recipient address, token details,
    /// and amount required.
    ///
    /// # Parameters
    ///
    /// - `pay_to`: The recipient address (can be any type convertible to [`ChecksummedAddress`])
    /// - `asset`: The token deployment and amount required
    ///
    /// # Returns
    ///
    /// A [`v1::PriceTag`] that can be included in a `PaymentRequired` response.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use x402_chain_eip155::{V1Eip155Exact, KnownNetworkEip155};
    /// use x402_types::networks::USDC;
    ///
    /// let usdc = USDC::base();
    /// let price_tag = V1Eip155Exact::price_tag(
    ///     "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb",
    ///     usdc.amount(1_000_000u64), // 1 USDC
    /// );
    /// ```
    ///
    /// # Panics
    ///
    /// Panics if the chain ID cannot be converted to a network name. This should
    /// only happen for unsupported or custom chains without registered network names.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn price_tag<A: Into<ChecksummedAddress>>(
        pay_to: A,
        asset: DeployedTokenAmount<U256, Eip155TokenDeployment>,
    ) -> v1::PriceTag {
        let chain_id: ChainId = asset.token.chain_reference.into();
        let network = chain_id
            .as_network_name()
            .unwrap_or_else(|| panic!("Can not get network name for chain id {}", chain_id));
        let extra = serde_json::to_value(asset.token.transfer_method).ok();
        v1::PriceTag {
            scheme: ExactScheme.to_string(),
            pay_to: pay_to.into().to_string(),
            asset: asset.token.address.to_string(),
            network: network.to_string(),
            amount: asset.amount.to_string(),
            max_timeout_seconds: 300,
            extra,
            enricher: None,
        }
    }
}
