use std::sync::Arc;
use x402_types::chain::{ChainId, DeployedTokenAmount};
use x402_types::proto;
use x402_types::proto::v2;

use crate::V2SolanaExact;
use crate::chain::{Address, SolanaTokenDeployment};
use crate::v1_solana_exact::SupportedPaymentKindExtra;
use crate::v1_solana_exact::types::ExactScheme;

impl V2SolanaExact {
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn price_tag<A: Into<Address>>(
        pay_to: A,
        asset: DeployedTokenAmount<u64, SolanaTokenDeployment>,
    ) -> v2::PriceTag {
        let chain_id: ChainId = asset.token.chain_reference.into();
        let requirements = v2::PaymentRequirements {
            scheme: ExactScheme.to_string(),
            pay_to: pay_to.into().to_string(),
            asset: asset.token.address.to_string(),
            network: chain_id,
            amount: asset.amount.to_string(),
            max_timeout_seconds: 300,
            extra: None,
        };
        v2::PriceTag {
            requirements,
            enricher: Some(Arc::new(solana_fee_payer_enricher_v2)),
        }
    }
}

/// Enricher function for V2 Solana price tags - adds fee_payer to extra field
pub fn solana_fee_payer_enricher_v2(
    price_tag: &mut v2::PriceTag,
    capabilities: &proto::SupportedResponse,
) {
    if price_tag.requirements.extra.is_some() {
        return;
    }

    // Find the matching kind and deserialize the whole extra into SupportedPaymentKindExtra
    let extra = capabilities
        .kinds
        .iter()
        .find(|kind| {
            v2::X402Version2 == kind.x402_version
                && kind.scheme == ExactScheme.to_string()
                && kind.network == price_tag.requirements.network.to_string()
        })
        .and_then(|kind| kind.extra.as_ref())
        .and_then(|extra| serde_json::from_value::<SupportedPaymentKindExtra>(extra.clone()).ok());

    // Serialize the whole extra back to Value
    if let Some(extra) = extra {
        price_tag.requirements.extra = serde_json::to_value(&extra).ok();
    }
}
