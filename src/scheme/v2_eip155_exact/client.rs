use crate::chain::eip155::Eip155ChainReference;
use crate::proto::client::{PaymentCandidate, PaymentCandidateSigner, X402Error, X402SchemeClient};
use crate::proto::v2::ResourceInfo;
use crate::proto::{PaymentRequired, v2};
use crate::scheme::X402SchemeId;
use crate::scheme::v1_eip155_exact::client::{
    Eip3009SigningParams, SignerLike, sign_erc3009_authorization,
};
use crate::scheme::v2_eip155_exact::V2Eip155Exact;
use crate::scheme::v2_eip155_exact::types;
use crate::util::Base64Bytes;
use async_trait::async_trait;
use serde::Deserialize;

#[derive(Debug)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct V2Eip155ExactClient<S> {
    signer: S,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl<S> V2Eip155ExactClient<S> {
    pub fn new(signer: S) -> Self {
        Self { signer }
    }
}

impl<S> X402SchemeId for V2Eip155ExactClient<S> {
    fn namespace(&self) -> &str {
        V2Eip155Exact.namespace()
    }

    fn scheme(&self) -> &str {
        V2Eip155Exact.scheme()
    }
}

impl<S> X402SchemeClient for V2Eip155ExactClient<S>
where
    S: SignerLike + Clone + Send + Sync + 'static,
{
    fn accept(&self, payment_required: &PaymentRequired) -> Vec<PaymentCandidate> {
        let payment_required = match payment_required {
            PaymentRequired::V2(payment_required) => payment_required,
            PaymentRequired::V1(_) => {
                return vec![];
            }
        };
        payment_required
            .accepts
            .iter()
            .filter_map(|v| {
                let requirements = types::PaymentRequirements::deserialize(v).ok()?;
                let chain_reference = Eip155ChainReference::try_from(&requirements.network).ok()?;
                let candidate = PaymentCandidate {
                    chain_id: requirements.network.clone(),
                    asset: requirements.asset.to_string(),
                    amount: requirements.amount.into(),
                    scheme: self.scheme().to_string(),
                    x402_version: self.x402_version(),
                    pay_to: requirements.pay_to.to_string(),
                    signer: Box::new(PayloadSigner {
                        resource_info: payment_required.resource.clone(),
                        signer: self.signer.clone(),
                        chain_reference,
                        requirements,
                    }),
                };
                Some(candidate)
            })
            .collect::<Vec<_>>()
    }
}

#[allow(dead_code)] // Public for consumption by downstream crates.
struct PayloadSigner<S> {
    signer: S,
    resource_info: ResourceInfo,
    chain_reference: Eip155ChainReference,
    requirements: types::PaymentRequirements,
}

#[async_trait]
impl<S> PaymentCandidateSigner for PayloadSigner<S>
where
    S: Sync + SignerLike,
{
    async fn sign_payment(&self) -> Result<String, X402Error> {
        let params = Eip3009SigningParams {
            chain_id: self.chain_reference.inner(),
            asset_address: self.requirements.asset.0,
            pay_to: self.requirements.pay_to.into(),
            amount: self.requirements.amount.into(),
            max_timeout_seconds: self.requirements.max_timeout_seconds,
            extra: self.requirements.extra.clone(),
        };

        let evm_payload = sign_erc3009_authorization(&self.signer, &params).await?;

        // Build the payment payload
        let payload = types::PaymentPayload {
            x402_version: v2::X402Version2,
            accepted: self.requirements.clone(),
            resource: self.resource_info.clone(),
            payload: evm_payload,
        };
        let json = serde_json::to_vec(&payload)?;
        let b64 = Base64Bytes::encode(&json);

        Ok(b64.to_string())
    }
}
