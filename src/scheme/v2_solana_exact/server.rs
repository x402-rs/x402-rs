use std::sync::Arc;

use crate::chain::solana::{Address, SolanaTokenDeployment};
use crate::chain::{ChainId, DeployedTokenAmount};
use crate::proto::SupportedResponse;
use crate::proto::v2;
use crate::scheme::IntoPriceTag;
use crate::scheme::v1_solana_exact::ExactScheme;
use crate::scheme::v1_solana_exact::types::SupportedPaymentKindExtra;

#[derive(Debug, Clone)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct V2SolanaExactPriceTag {
    pub pay_to: Address,
    pub asset: DeployedTokenAmount<u64, SolanaTokenDeployment>,
    pub max_timeout_seconds: u64,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl V2SolanaExactPriceTag {
    pub fn new(pay_to: Address, asset: DeployedTokenAmount<u64, SolanaTokenDeployment>) -> Self {
        Self {
            pay_to,
            asset,
            max_timeout_seconds: 300,
        }
    }

    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.max_timeout_seconds = seconds;
        self
    }
}

impl IntoPriceTag for V2SolanaExactPriceTag {
    type PriceTag = v2::PriceTag;

    fn into_price_tag(self) -> Self::PriceTag {
        let chain_id: ChainId = self.asset.token.chain_reference.into();
        let requirements = v2::PaymentRequirements {
            scheme: ExactScheme.to_string(),
            pay_to: self.pay_to.to_string(),
            asset: self.asset.token.address.to_string(),
            network: chain_id,
            amount: self.asset.amount.to_string(),
            max_timeout_seconds: self.max_timeout_seconds,
            extra: None,
        };
        v2::PriceTag {
            requirements,
            enricher: Some(Arc::new(solana_fee_payer_enricher_v2)),
        }
    }
}

/// Enricher function for V2 Solana price tags - adds fee_payer to extra field
#[allow(dead_code)]
fn solana_fee_payer_enricher_v2(price_tag: &mut v2::PriceTag, capabilities: &SupportedResponse) {
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
