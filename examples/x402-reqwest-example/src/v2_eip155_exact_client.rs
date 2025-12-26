use alloy_primitives::{Address, U256};
use alloy_signer::Signer;
use serde::Deserialize;
use x402_rs::chain::ChainId;
use x402_rs::chain::eip155::ChecksummedAddress;
use x402_rs::proto::client::PaymentCandidateLike;
use x402_rs::proto::v2::ResourceInfo;
use x402_rs::proto::{PaymentRequired, v2};
use x402_rs::scheme::X402SchemeId;
use x402_rs::scheme::v2_eip155_exact;

use crate::client::{PaymentCandidate, X402SchemeClient};

#[derive(Debug)]
pub struct V2Eip155ExactClient<S> {
    signer: S,
}

impl<S> X402SchemeId for V2Eip155ExactClient<S> {
    fn namespace(&self) -> &str {
        "eip155"
    }

    fn scheme(&self) -> &str {
        "exact"
    }
}

impl<S> V2Eip155ExactClient<S> {
    pub fn new(signer: S) -> Self {
        Self { signer }
    }
}

impl<S> From<S> for V2Eip155ExactClient<S>
where
    S: Signer + Send + Sync,
{
    fn from(signer: S) -> Self {
        Self::new(signer)
    }
}

struct Accepted {
    chain_id: ChainId,
    asset: String,
    amount: U256,
    scheme: String,
    x402_version: u8,
    pay_to: String,
}

impl PaymentCandidateLike for Accepted {
    fn chain_id(&self) -> &ChainId {
        &self.chain_id
    }

    fn asset(&self) -> &str {
        &self.asset
    }

    fn amount(&self) -> U256 {
        self.amount
    }

    fn scheme(&self) -> &str {
        &self.scheme
    }

    fn x402_version(&self) -> u8 {
        self.x402_version
    }

    fn pay_to(&self) -> &str {
        &self.pay_to
    }
}

impl<S> X402SchemeClient for V2Eip155ExactClient<S>
where
    S: Signer + Send + Sync,
{
    fn accept(
        &self,
        payment_required: &PaymentRequired,
    ) -> Vec<Box<dyn PaymentCandidateLike>> {
        let payment_required = match payment_required {
            PaymentRequired::V2(payment_required) => payment_required,
            PaymentRequired::V1(_) => {
                todo!("Reject V1 requests for v2 EIP-155 exact scheme")
            }
        };
        payment_required
            .accepts
            .iter()
            .filter_map(|v| v2_eip155_exact::PaymentRequirements::deserialize(v).ok())
            .map(|requirements| {
                let accepted = Accepted {
                    chain_id: requirements.network,
                    asset: requirements.asset.to_string(),
                    amount: requirements.amount,
                    scheme: requirements.scheme.to_string(),
                    x402_version: self.x402_version(),
                    pay_to: requirements.pay_to.to_string(),
                };
                Box::new(accepted) as Box<dyn PaymentCandidateLike>
            })
            .collect::<Vec<_>>()
    }
}
