use alloy_primitives::{FixedBytes, U256};
use alloy_signer::Signer;
use alloy_sol_types::{eip712_domain, SolStruct};
use async_trait::async_trait;
use rand::{rng, Rng};
use serde::Deserialize;

use crate::chain::eip155::Eip155ChainReference;
use crate::proto::client::{PaymentCandidate, PaymentCandidateSigner, X402Error, X402SchemeClient};
use crate::proto::{v2, PaymentRequired};
use crate::proto::v2::ResourceInfo;
use crate::scheme::v1_eip155_exact::{ExactEvmPayload, ExactEvmPayloadAuthorization, TransferWithAuthorization};
use crate::scheme::v2_eip155_exact::types;
use crate::scheme::v2_eip155_exact::V2Eip155Exact;
use crate::scheme::X402SchemeId;
use crate::timestamp::UnixTimestamp;
use crate::util::Base64Bytes;

#[derive(Debug)]
pub struct V2Eip155ExactClient<S> {
    signer: S,
}

impl<S> From<S> for V2Eip155ExactClient<S>
where
    S: Signer + Send + Sync,
{
    fn from(signer: S) -> Self {
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
    S: Signer + Clone + Send + Sync + 'static,
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

struct PayloadSigner<S> {
    signer: S,
    resource_info: ResourceInfo,
    chain_reference: Eip155ChainReference,
    requirements: types::PaymentRequirements,
}

#[async_trait]
impl<S> PaymentCandidateSigner for PayloadSigner<S>
where
    S: Sync + Signer,
{
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
            verifying_contract: self.requirements.asset.0,
        };
        // Build authorization
        let now = UnixTimestamp::now();
        // valid_after should be in the past (10 minutes ago) to ensure the payment is immediately valid
        let valid_after_secs = now.as_secs().saturating_sub(10 * 60);
        let valid_after = UnixTimestamp::from_secs(valid_after_secs);
        let valid_before = now + self.requirements.max_timeout_seconds;
        let nonce: [u8; 32] = rng().random();
        let nonce = FixedBytes(nonce);

        let authorization = ExactEvmPayloadAuthorization {
            from: self.signer.address(),
            to: self.requirements.pay_to.into(),
            value: self.requirements.amount.into(),
            valid_after,
            valid_before,
            nonce,
        };

        // Create the EIP-712 struct for signing
        // IMPORTANT: The values here MUST match the authorization struct exactly,
        // as the facilitator will reconstruct this struct from the authorization
        // to verify the signature.
        let transfer_with_authorization = TransferWithAuthorization {
            from: authorization.from,
            to: authorization.to,
            value: authorization.value,
            validAfter: U256::from(authorization.valid_after.as_secs()),
            validBefore: U256::from(authorization.valid_before.as_secs()),
            nonce: authorization.nonce,
        };

        let eip712_hash = transfer_with_authorization.eip712_signing_hash(&domain);
        let signature = self
            .signer
            .sign_hash(&eip712_hash)
            .await
            .map_err(|e| X402Error::SigningError(format!("{e:?}")))?;

        // Build the payment payload
        let payload = types::PaymentPayload {
            x402_version: v2::X402Version2,
            accepted: self.requirements.clone(),
            resource: self.resource_info.clone(),
            payload: ExactEvmPayload {
                signature: signature.as_bytes().into(),
                authorization,
            },
        };
        let json = serde_json::to_vec(&payload)?;
        let b64 = Base64Bytes::encode(&json);

        Ok(b64.to_string())
    }
}
