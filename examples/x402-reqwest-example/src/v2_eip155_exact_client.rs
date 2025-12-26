use alloy_primitives::{Address, FixedBytes, U256};
use alloy_signer::Signer;
use alloy_sol_types::eip712_domain;
use async_trait::async_trait;
use rand::{rng, Rng};
use serde::Deserialize;
use x402_rs::chain::ChainId;
use x402_rs::chain::eip155::Eip155ChainReference;
use x402_rs::proto::{PaymentRequired};
use x402_rs::proto::client::{PaymentCandidate, PaymentCandidateSigner, X402Error};
use x402_rs::scheme::v1_eip155_exact::ExactEvmPayloadAuthorization;
use x402_rs::scheme::X402SchemeId;
use x402_rs::scheme::v2_eip155_exact;
use x402_rs::timestamp::UnixTimestamp;
use x402_rs::util::Base64Bytes;
use crate::client::{X402SchemeClient};

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

struct PayloadSigner {
    chain_reference: Eip155ChainReference,
    requirements: v2_eip155_exact::PaymentRequirements,
}

#[async_trait]
impl PaymentCandidateSigner for PayloadSigner {
    async fn sign_payment(&self) -> Result<String, X402Error> {
        let (name, version) = match &self.requirements.extra {
            None => ("".to_string(), "".to_string()),
            Some(extra) => (extra.name.clone(), extra.version.clone()),
        };
        let chain_id_num = self.chain_reference.inner();
        // Build EIP-712 domain
        let domain = eip712_domain! {
            name: name,
            version: version,
            chain_id: chain_id_num,
            verifying_contract: self.requirements.asset,
        };

        // Build authorization
        let now = UnixTimestamp::now();
        // valid_after should be in the past (10 minutes ago) to ensure the payment is immediately valid
        let valid_after_secs = now.as_secs().saturating_sub(10 * 60);
        let valid_after = UnixTimestamp::from_secs(valid_after_secs);
        let valid_before = now + self.requirements.max_timeout_seconds;
        let nonce: [u8; 32] = rng().random();

        // let authorization = ExactEvmPayloadAuthorization {
        //     from: self.signer.address().into(),
        //     to: self.requirements.pay_to,
        //     value: self.requirements.amount,
        //     valid_after,
        //     valid_before,
        //     nonce: FixedBytes(nonce),
        // };

        todo!("Sign payload using signer")
    }
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
                let chain_reference = Eip155ChainReference::try_from(&requirements.network).ok()?;
                let candidate = PaymentCandidate {
                    chain_id: requirements.network.clone(),
                    asset: requirements.asset.to_string(),
                    amount: requirements.amount,
                    scheme: self.scheme().to_string(),
                    x402_version: self.x402_version(),
                    pay_to: requirements.pay_to.to_string(),
                    signer: Box::new(PayloadSigner {
                        chain_reference,
                        requirements
                    }),
                };
                Some(candidate)
            })
            .collect::<Vec<_>>()
    }
}
