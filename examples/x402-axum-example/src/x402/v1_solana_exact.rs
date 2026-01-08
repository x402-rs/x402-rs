use x402_rs::chain::solana::{Address, SolanaTokenDeployment};
use x402_rs::chain::{ChainId, DeployedTokenAmount};
use x402_rs::proto::server::IntoPriceTag;
use x402_rs::proto::v1::V1PriceTag;

#[derive(Debug, Clone)]
pub struct V1SolanaExactSchemePriceTag {
    pub pay_to: Address,
    pub asset: DeployedTokenAmount<u64, SolanaTokenDeployment>,
    pub max_timeout_seconds: u64,
}

impl IntoPriceTag for V1SolanaExactSchemePriceTag {
    type PriceTag = V1PriceTag;

    fn into_price_tag(self) -> Self::PriceTag {
        let chain_id: ChainId = self.asset.token.chain_reference.into();
        let network = chain_id
            .as_network_name()
            .expect(format!("Can not get network name for chain id {}", chain_id).as_str());
        V1PriceTag {
            scheme: "exact".to_string(),
            pay_to: self.pay_to.to_string(),
            asset: self.asset.token.address.to_string(),
            network: network.to_string(),
            amount: self.asset.amount.to_string(),
            max_timeout_seconds: self.max_timeout_seconds,
            extra: None,
        }
    }
}
