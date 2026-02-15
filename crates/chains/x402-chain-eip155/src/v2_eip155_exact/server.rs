//! Server-side price tag generation for V2 EIP-155 exact scheme.
//!
//! This module provides functionality for servers to create V2 price tags
//! that clients can use to generate payment authorizations. V2 uses CAIP-2
//! chain IDs instead of network names.

use alloy_primitives::U256;
use serde_json::json;
use x402_types::chain::{ChainId, DeployedTokenAmount};
use x402_types::proto::v2;

use crate::V2Eip155Exact;
use crate::chain::{AssetTransferMethod, ChecksummedAddress, Eip155TokenDeployment};
use crate::v1_eip155_exact::ExactScheme;

impl V2Eip155Exact {
    /// Creates a V2 price tag for an ERC-3009 payment on an EVM chain.
    ///
    /// This function generates a V2 price tag that specifies the payment requirements
    /// for a resource. Unlike V1, V2 uses CAIP-2 chain IDs (e.g., `eip155:8453`) instead
    /// of network names, and embeds the requirements directly in the price tag.
    ///
    /// # Parameters
    ///
    /// - `pay_to`: The recipient address (can be any type convertible to [`ChecksummedAddress`])
    /// - `asset`: The token deployment and amount required
    ///
    /// # Returns
    ///
    /// A [`v2::PriceTag`] that can be included in a `PaymentRequired` response.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use x402_chain_eip155::{V2Eip155Exact, KnownNetworkEip155};
    /// use x402_types::networks::USDC;
    ///
    /// let usdc = USDC::base();
    /// let price_tag = V2Eip155Exact::price_tag(
    ///     "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb",
    ///     usdc.amount(1_000_000u64), // 1 USDC
    /// );
    /// ```
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn price_tag<A: Into<ChecksummedAddress>>(
        pay_to: A,
        asset: DeployedTokenAmount<U256, Eip155TokenDeployment>,
    ) -> v2::PriceTag {
        let chain_id: ChainId = asset.token.chain_reference.into();
        let extra = match asset.token.asset_transfer_method {
            AssetTransferMethod::Eip3009 => {
                asset
                    .token
                    .eip712
                    .and_then(|eip712| serde_json::to_value(&eip712).ok())
            }
            AssetTransferMethod::Permit2 => {
                serde_json::to_value(json!({
                    "assetTransferMethod": "permit2" // TODO: Use some shared struct for that
                })).ok()
            }
        };
        let requirements = v2::PaymentRequirements {
            scheme: ExactScheme.to_string(),
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
