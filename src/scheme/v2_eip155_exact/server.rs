use crate::chain::eip155::{ChecksummedAddress, Eip155TokenDeployment};
use crate::chain::{ChainId, DeployedTokenAmount};
use crate::proto::v2;
use crate::scheme::IntoPriceTag;
use crate::scheme::v2_eip155_exact::ExactScheme;
use alloy_primitives::U256;

#[derive(Debug, Clone)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct V2Eip155ExactPriceTag {
    pub pay_to: ChecksummedAddress,
    pub asset: DeployedTokenAmount<U256, Eip155TokenDeployment>,
    pub max_timeout_seconds: u64,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl V2Eip155ExactPriceTag {
    pub fn new(
        pay_to: ChecksummedAddress,
        asset: DeployedTokenAmount<U256, Eip155TokenDeployment>,
    ) -> Self {
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

impl IntoPriceTag for V2Eip155ExactPriceTag {
    type PriceTag = v2::PriceTag;

    fn into_price_tag(self) -> Self::PriceTag {
        let chain_id: ChainId = self.asset.token.chain_reference.into();
        let extra = self
            .asset
            .token
            .eip712
            .and_then(|eip712| serde_json::to_string(&eip712).ok())
            .and_then(|extra| serde_json::value::RawValue::from_string(extra).ok());
        v2::PaymentRequirements {
            scheme: ExactScheme.to_string(),
            pay_to: self.pay_to.to_string(),
            asset: self.asset.token.address.to_string(),
            network: chain_id,
            amount: self.asset.amount.to_string(),
            max_timeout_seconds: self.max_timeout_seconds,
            extra,
        }
    }
}
