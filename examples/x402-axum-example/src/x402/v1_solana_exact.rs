use x402_rs::chain::solana::{Address, SolanaTokenDeployment};
use x402_rs::chain::{ChainId, DeployedTokenAmount};
use x402_rs::proto::server::IntoPriceTag;
use x402_rs::proto::v1::V1PriceTag;
use x402_rs::scheme::v1_solana_exact::types::SupportedPaymentKindExtra;

#[derive(Debug, Clone)]
pub struct V1SolanaExactSchemePriceTag {
    pub pay_to: Address,
    pub asset: DeployedTokenAmount<u64, SolanaTokenDeployment>,
    pub fee_payer: Address,
}

impl IntoPriceTag for V1SolanaExactSchemePriceTag {
    type PriceTag = V1PriceTag;

    fn into_price_tag(self) -> Self::PriceTag {
        let chain_id: ChainId = self.asset.token.chain_reference.into();
        let network = chain_id
            .as_network_name()
            .expect(format!("Can not get network name for chain id {}", chain_id).as_str());
        let extra = serde_json::to_string(&SupportedPaymentKindExtra {
            fee_payer: self.fee_payer,
        })
        .ok()
        .and_then(|extra| serde_json::value::RawValue::from_string(extra).ok());
        V1PriceTag {
            scheme: "exact".to_string(),
            pay_to: self.pay_to.to_string(),
            asset: self.asset.token.address.to_string(),
            network: network.to_string(),
            amount: self.asset.amount.to_string(),
            max_timeout_seconds: 300,
            extra,
        }
    }
}
