use x402_rs::__reexports::alloy_primitives::U256;
use x402_rs::chain::eip155::Eip155TokenDeployment;
use x402_rs::chain::{ChainId, DeployedTokenAmount, eip155};
use x402_rs::proto::v2;
use x402_rs::scheme::IntoPriceTag;

#[derive(Debug, Clone)]
pub struct V2Eip155ExactSchemePriceTag {
    pub pay_to: eip155::ChecksummedAddress,
    pub asset: DeployedTokenAmount<U256, Eip155TokenDeployment>,
    pub max_timeout_seconds: u64,
}

impl IntoPriceTag for V2Eip155ExactSchemePriceTag {
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
            scheme: "exact".to_string(),
            pay_to: self.pay_to.to_string(),
            asset: self.asset.token.address.to_string(),
            network: chain_id,
            amount: self.asset.amount.to_string(),
            max_timeout_seconds: self.max_timeout_seconds,
            extra,
        }
    }
}
