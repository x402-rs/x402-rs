use x402_rs::__reexports::alloy_primitives::U256;
use x402_rs::chain::eip155::Eip155TokenDeployment;
use x402_rs::chain::{ChainId, DeployedTokenAmount, eip155};
use x402_rs::proto::server::IntoPriceTag;
use x402_rs::proto::v1::V1PriceTag;

#[derive(Debug, Clone)]
pub struct V1Eip155ExactSchemePriceTag {
    pub pay_to: eip155::ChecksummedAddress,
    pub asset: DeployedTokenAmount<U256, Eip155TokenDeployment>,
}

impl IntoPriceTag for V1Eip155ExactSchemePriceTag {
    type PriceTag = V1PriceTag;

    fn into_price_tag(self) -> Self::PriceTag {
        let chain_id: ChainId = self.asset.token.chain_reference.into();
        let network = chain_id
            .as_network_name()
            .expect(format!("Can not get network name for chain id {}", chain_id).as_str());
        let extra = self
            .asset
            .token
            .eip712
            .and_then(|eip712| serde_json::to_value(eip712).ok());
        V1PriceTag {
            scheme: "exact".to_string(), // FIXME
            pay_to: self.pay_to.to_string(),
            asset: self.asset.token.address.to_string(),
            network: network.to_string(),
            amount: self.asset.amount.to_string(),
            max_timeout_seconds: 300,
            extra,
        }
    }
}
