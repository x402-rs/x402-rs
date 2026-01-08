use alloy_primitives::U256;

use crate::chain::eip155::{ChecksummedAddress, Eip155TokenDeployment};
use crate::chain::{ChainId, DeployedTokenAmount};
use crate::proto::v1;
use crate::scheme::IntoPriceTag;
use crate::scheme::v1_eip155_exact::ExactScheme;

#[derive(Debug, Clone)]
pub struct V1Eip155ExactPriceTag {
    pub pay_to: ChecksummedAddress,
    pub asset: DeployedTokenAmount<U256, Eip155TokenDeployment>,
    pub max_timeout_seconds: u64,
}

impl V1Eip155ExactPriceTag {
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

impl IntoPriceTag for V1Eip155ExactPriceTag {
    type PriceTag = v1::PriceTag;

    fn into_price_tag(self) -> Self::PriceTag {
        let chain_id: ChainId = self.asset.token.chain_reference.into();
        let network = chain_id
            .as_network_name()
            .expect(format!("Can not get network name for chain id {}", chain_id).as_str());
        let extra = self
            .asset
            .token
            .eip712
            .and_then(|eip712| serde_json::to_string(&eip712).ok())
            .and_then(|extra| serde_json::value::RawValue::from_string(extra).ok());
        v1::PriceTag {
            scheme: ExactScheme.to_string(),
            pay_to: self.pay_to.to_string(),
            asset: self.asset.token.address.to_string(),
            network: network.to_string(),
            amount: self.asset.amount.to_string(),
            max_timeout_seconds: self.max_timeout_seconds,
            extra,
        }
    }
}
