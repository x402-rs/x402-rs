use alloy_primitives::U256;
use x402_types::chain::{ChainId, DeployedTokenAmount};
use x402_types::proto::v1;

use crate::V1Eip155Exact;
use crate::chain::{ChecksummedAddress, Eip155TokenDeployment};
use crate::v1_eip155_exact::ExactScheme;

impl V1Eip155Exact {
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn price_tag<A: Into<ChecksummedAddress>>(
        pay_to: A,
        asset: DeployedTokenAmount<U256, Eip155TokenDeployment>,
    ) -> v1::PriceTag {
        let chain_id: ChainId = asset.token.chain_reference.into();
        let network = chain_id
            .as_network_name()
            .unwrap_or_else(|| panic!("Can not get network name for chain id {}", chain_id));
        let extra = asset
            .token
            .eip712
            .and_then(|eip712| serde_json::to_value(&eip712).ok());
        v1::PriceTag {
            scheme: ExactScheme.to_string(),
            pay_to: pay_to.into().to_string(),
            asset: asset.token.address.to_string(),
            network: network.to_string(),
            amount: asset.amount.to_string(),
            max_timeout_seconds: 300,
            extra,
            enricher: None,
        }
    }
}
