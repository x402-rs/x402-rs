//! Server-side price tag generation for V2 EIP-155 upto scheme.
//!
//! This module provides functionality for servers to create V2 price tags
//! for the "upto" payment scheme. Unlike the exact scheme, upto allows clients
//! to authorize a maximum amount, with the actual settled amount determined
//! at settlement time based on resource consumption.
//!
//! # Note on Extra Field
//!
//! The upto scheme is Permit2-only and does not require token name/version in the
//! extra field. Permit2's EIP-712 domain always uses `name: "Permit2"` regardless
//! of the token. The `UptoExtra` type exists for type compatibility but its fields
//! are not consumed by the client or facilitator.

use alloy_primitives::U256;
use x402_types::chain::{ChainId, DeployedTokenAmount};
use x402_types::proto::v2;

use crate::V2Eip155Upto;
use crate::chain::{ChecksummedAddress, Eip155TokenDeployment};
use crate::v2_eip155_upto::types::UptoScheme;

impl V2Eip155Upto {
    /// Creates a V2 price tag for an upto payment on an EVM chain.
    ///
    /// This function generates a V2 price tag that specifies the maximum payment
    /// amount for a resource. The client will authorize up to this amount, and the
    /// server can settle for any amount less than or equal to the maximum based on
    /// actual resource consumption.
    ///
    /// # Parameters
    ///
    /// - `pay_to`: The recipient address (can be any type convertible to [`ChecksummedAddress`])
    /// - `asset`: The token deployment and maximum amount authorized
    ///
    /// # Returns
    ///
    /// A [`v2::PriceTag`] that can be included in a `PaymentRequired` response.
    ///
    /// # Example
    ///
    /// ```ignore
    /// use x402_chain_eip155::{V2Eip155Upto, KnownNetworkEip155};
    /// use x402_types::networks::USDC;
    ///
    /// let usdc = USDC::base();
    /// let price_tag = V2Eip155Upto::price_tag(
    ///     "0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb",
    ///     usdc.amount(5_000_000u64), // Up to 5 USDC
    /// );
    /// ```
    // TODO Server support for upto variable scheme is missing
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn price_tag<A: Into<ChecksummedAddress>>(
        pay_to: A,
        asset: DeployedTokenAmount<U256, Eip155TokenDeployment>,
    ) -> v2::PriceTag {
        let chain_id: ChainId = asset.token.chain_reference.into();

        // Upto scheme is Permit2-only and doesn't need extra data
        // The EIP-712 domain for Permit2 is always "Permit2", not the token's name/version
        let requirements = v2::PaymentRequirements {
            scheme: UptoScheme.to_string(),
            pay_to: pay_to.into().to_string(),
            asset: asset.token.address.to_string(),
            network: chain_id,
            amount: asset.amount.to_string(),
            max_timeout_seconds: 300,
            extra: None,
        };
        v2::PriceTag {
            requirements,
            enricher: None,
        }
    }
}
