//! Server-side price tag generation for V2 EIP-155 upto scheme.
//!
//! The upto price tag includes an enricher that injects the facilitator's address
//! into the payment requirements `extra` field. The client reads this address and
//! embeds it in the Permit2 witness so only the authorized facilitator can settle.

use std::sync::Arc;

use alloy_primitives::U256;
use x402_types::chain::{ChainId, DeployedTokenAmount};
use x402_types::proto;
use x402_types::proto::v2;

use crate::V2Eip155Upto;
use crate::chain::{ChecksummedAddress, Eip155TokenDeployment};
use crate::v2_eip155_upto::types::UptoScheme;

impl V2Eip155Upto {
    /// Creates a V2 price tag for an upto payment on an EVM chain.
    ///
    /// The returned price tag includes an enricher that populates
    /// `extra.facilitatorAddress` from the facilitator's `supported()` response,
    /// which is required by the client when signing the Permit2 witness.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn price_tag<A: Into<ChecksummedAddress>>(
        pay_to: A,
        asset: DeployedTokenAmount<U256, Eip155TokenDeployment>,
    ) -> v2::PriceTag {
        let chain_id: ChainId = asset.token.chain_reference.into();
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
            enricher: Some(Arc::new(upto_facilitator_address_enricher)),
        }
    }
}

/// Enricher that copies `facilitatorAddress` from the facilitator's `supported()` extra
/// into the price tag's payment requirements extra field.
pub fn upto_facilitator_address_enricher(
    price_tag: &mut v2::PriceTag,
    capabilities: &proto::SupportedResponse,
) {
    if price_tag.requirements.extra.is_some() {
        return;
    }

    let supported_extra = capabilities
        .kinds
        .iter()
        .find(|kind| {
            v2::X402Version2 == kind.x402_version
                && kind.scheme == UptoScheme.to_string()
                && kind.network == price_tag.requirements.network.to_string()
        })
        .and_then(|kind| kind.extra.clone());

    price_tag.requirements.extra = supported_extra;
}
