use alloy_signer::Signer;
use serde::Deserialize;
use x402_rs::proto::v2::ResourceInfo;
use x402_rs::proto::{PaymentRequired, v2};
use x402_rs::scheme::X402SchemeId;
use x402_rs::scheme::v2_eip155_exact;

use crate::client::{PaymentCandidate, X402SchemeClient, AcceptedRequestLike};

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

struct Accepted<'a, S> {
    payment_required: &'a v2::PaymentRequired,
    accepts: Vec<v2_eip155_exact::PaymentRequirements>,
    client: &'a V2Eip155ExactClient<S>,
}

impl<'a, S> AcceptedRequestLike<'a> for Accepted<'a, S> {
    fn candidates(&self) -> Vec<PaymentCandidate<'a>> {
        vec![]
    }
}

impl<S> X402SchemeClient for V2Eip155ExactClient<S>
where
    S: Signer + Send + Sync,
{
    fn accept<'a>(&'a self, payment_required: &'a PaymentRequired) -> Box<dyn AcceptedRequestLike<'a> + 'a> {
        let payment_required = match payment_required {
            PaymentRequired::V2(payment_required) => payment_required,
            PaymentRequired::V1(_) => {
                todo!("Reject V1 requests for v2 EIP-155 exact scheme")
            }
        };
        let accepts = payment_required
            .accepts
            .iter()
            .filter_map(|v| v2_eip155_exact::PaymentRequirements::deserialize(v).ok())
            .collect::<Vec<_>>();
        println!("Accepts: {:?}", accepts);
        Box::new(Accepted {
            payment_required,
            accepts,
            client: self,
        })
    }
}
