use x402_rs::chain::solana::{Address, SolanaTokenDeployment};
use x402_rs::chain::{ChainId, DeployedTokenAmount};
use x402_rs::proto::server::IntoPriceTag;
use x402_rs::proto::v2;

#[derive(Debug, Clone)]
pub struct V2SolanaExactSchemePriceTag {
    pub pay_to: Address,
    pub asset: DeployedTokenAmount<u64, SolanaTokenDeployment>,
    pub max_timeout_seconds: u64,
}

/// V2 price tag type for use with the middleware
pub type V2PriceTag = v2::PaymentRequirements;

impl IntoPriceTag for V2SolanaExactSchemePriceTag {
    type PriceTag = V2PriceTag;

    fn into_price_tag(self) -> Self::PriceTag {
        let chain_id: ChainId = self.asset.token.chain_reference.into();
        v2::PaymentRequirements {
            scheme: "exact".to_string(),
            pay_to: self.pay_to.to_string(),
            asset: self.asset.token.address.to_string(),
            network: chain_id,
            amount: self.asset.amount.to_string(),
            max_timeout_seconds: self.max_timeout_seconds,
            extra: None,
        }
    }
}
