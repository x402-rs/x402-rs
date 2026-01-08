use crate::chain::solana::{Address, SolanaTokenDeployment};
use crate::chain::{ChainId, DeployedTokenAmount};
use crate::proto::v2;
use crate::scheme::IntoPriceTag;

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
