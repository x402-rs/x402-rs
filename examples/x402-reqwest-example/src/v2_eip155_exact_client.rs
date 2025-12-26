use alloy_primitives::{Address, U256};
use alloy_signer::Signer;
use async_trait::async_trait;
use serde::Deserialize;
use x402_rs::chain::ChainId;
use x402_rs::proto::{PaymentRequired};
use x402_rs::proto::client::{ PaymentCandidate};
use x402_rs::scheme::X402SchemeId;
use x402_rs::scheme::v2_eip155_exact;
use x402_rs::util::Base64Bytes;
use crate::client::{ X402SchemeClient};

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

struct Candidate {
    requirements: v2_eip155_exact::PaymentRequirements,
    chain_id: ChainId,
    asset: String,
    amount: U256,
    scheme: String,
    x402_version: u8,
    pay_to: String,
}

impl<S> X402SchemeClient for V2Eip155ExactClient<S>
where
    S: Signer + Send + Sync,
{
    fn accept(&self, payment_required: &PaymentRequired) -> Vec<PaymentCandidate> {
        let payment_required = match payment_required {
            PaymentRequired::V2(payment_required) => payment_required,
            PaymentRequired::V1(_) => {
                todo!("Reject V1 requests for v2 EIP-155 exact scheme")
            }
        };
        payment_required
            .accepts
            .iter()
            .filter_map(|v| {
                let requirements = v2_eip155_exact::PaymentRequirements::deserialize(v).ok()?;
                let candidate = PaymentCandidate {
                    chain_id: requirements.network.clone(),
                    asset: requirements.asset.to_string(),
                    amount: requirements.amount,
                    scheme: self.scheme().to_string(),
                    x402_version: self.x402_version(),
                    pay_to: requirements.pay_to.to_string(),
                };
                Some(candidate)
            })
            .collect::<Vec<_>>()
    }
}
