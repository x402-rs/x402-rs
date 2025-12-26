use alloy_primitives::{FixedBytes, U256};
use alloy_signer::Signer;
use alloy_sol_types::{SolStruct, eip712_domain, sol};
use async_trait::async_trait;
use rand::{Rng, rng};
use serde::{Deserialize, Serialize};
use x402_rs::chain::eip155::Eip155ChainReference;
use x402_rs::proto::client::{PaymentCandidate, PaymentCandidateSigner, X402Error, X402SchemeClient};
use x402_rs::proto::v2::ResourceInfo;
use x402_rs::proto::{PaymentRequired, v2};
use x402_rs::scheme::X402SchemeId;
use x402_rs::scheme::v1_eip155_exact::{ExactEvmPayload, ExactEvmPayloadAuthorization};
use x402_rs::scheme::v2_eip155_exact;
use x402_rs::scheme::v2_eip155_exact::PaymentPayload;
use x402_rs::timestamp::UnixTimestamp;
use x402_rs::util::Base64Bytes;

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

struct PayloadSigner<S> {
    signer: S,
    resource_info: ResourceInfo,
    chain_reference: Eip155ChainReference,
    requirements: v2_eip155_exact::PaymentRequirements,
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

        let authorization = ExactEvmPayloadAuthorization {
            from: self.signer.address(),
            to: self.requirements.pay_to.into(),
            value: self.requirements.amount.into(),
            valid_after,
            valid_before,
            nonce: FixedBytes(nonce),
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
        let payload = PaymentPayload {
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

sol!(
    /// Solidity-compatible struct definition for ERC-3009 `transferWithAuthorization`.
    ///
    /// This matches the EIP-3009 format used in EIP-712 typed data:
    /// it defines the authorization to transfer tokens from `from` to `to`
    /// for a specific `value`, valid only between `validAfter` and `validBefore`
    /// and identified by a unique `nonce`.
    ///
    /// This struct is primarily used to reconstruct the typed data domain/message
    /// when verifying a client's signature.
    #[derive(Serialize, Deserialize)]
    struct TransferWithAuthorization {
        address from;
        address to;
        uint256 value;
        uint256 validAfter;
        uint256 validBefore;
        bytes32 nonce;
    }
);

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
                let requirements = v2_eip155_exact::PaymentRequirements::deserialize(v).ok()?;
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
