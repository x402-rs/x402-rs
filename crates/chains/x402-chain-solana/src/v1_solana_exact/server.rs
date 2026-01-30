use std::sync::Arc;
use x402_types::chain::{ChainId, DeployedTokenAmount};
use x402_types::proto;
use x402_types::proto::v1;

use crate::V1SolanaExact;
use crate::chain::{Address, SolanaTokenDeployment};
use crate::v1_solana_exact::types::{ExactScheme, SupportedPaymentKindExtra};

impl V1SolanaExact {
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn price_tag<A: Into<Address>>(
        pay_to: A,
        asset: DeployedTokenAmount<u64, SolanaTokenDeployment>,
    ) -> v1::PriceTag {
        let chain_id: ChainId = asset.token.chain_reference.into();
        let network = chain_id
            .as_network_name()
            .unwrap_or_else(|| panic!("Can not get network name for chain id {}", chain_id));
        v1::PriceTag {
            scheme: ExactScheme.to_string(),
            pay_to: pay_to.into().to_string(),
            asset: asset.token.address.to_string(),
            network: network.to_string(),
            amount: asset.amount.to_string(),
            max_timeout_seconds: 300,
            extra: None,
            enricher: Some(Arc::new(solana_fee_payer_enricher)),
        }
    }
}

/// Enricher function for Solana price tags - adds fee_payer to extra field
#[allow(dead_code)]
pub fn solana_fee_payer_enricher(
    price_tag: &mut v1::PriceTag,
    capabilities: &proto::SupportedResponse,
) {
    if price_tag.extra.is_some() {
        return;
    }

    // Find the matching kind and deserialize the whole extra into SupportedPaymentKindExtra
    let extra = capabilities
        .kinds
        .iter()
        .find(|kind| {
            v1::X402Version1 == kind.x402_version
                && kind.scheme == ExactScheme.to_string()
                && kind.network == price_tag.network
        })
        .and_then(|kind| kind.extra.as_ref())
        .and_then(|extra| serde_json::from_value::<SupportedPaymentKindExtra>(extra.clone()).ok());

    // Serialize the whole extra back to Value
    if let Some(extra) = extra {
        price_tag.extra = serde_json::to_value(&extra).ok();
    }
}
