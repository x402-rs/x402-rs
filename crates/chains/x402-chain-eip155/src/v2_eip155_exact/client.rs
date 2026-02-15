//! Client-side payment signing for the V2 EIP-155 "exact" scheme.
//!
//! This module provides [`V2Eip155ExactClient`] for signing ERC-3009
//! `transferWithAuthorization` payments on EVM chains using the V2 protocol.
//!
//! # Usage
//!
//! ```ignore
//! use x402_chain_eip155::v2_eip155_exact::client::V2Eip155ExactClient;
//! use alloy_signer_local::PrivateKeySigner;
//!
//! let signer = PrivateKeySigner::random();
//! let client = V2Eip155ExactClient::new(signer);
//! ```

use async_trait::async_trait;
use x402_types::proto::v2::ResourceInfo;
use x402_types::proto::{OriginalJson, PaymentRequired, v2};
use x402_types::scheme::X402SchemeId;
use x402_types::scheme::client::{
    PaymentCandidate, PaymentCandidateSigner, X402Error, X402SchemeClient,
};
use x402_types::util::Base64Bytes;

use crate::chain::{AssetTransferMethod, Eip155ChainReference};
use crate::v1_eip155_exact::PaymentRequirementsExtra;
use crate::v1_eip155_exact::client::{
    Eip3009SigningParams, SignerLike, sign_erc3009_authorization,
};
use crate::v2_eip155_exact::types;
use crate::v2_eip155_exact::{ExactEvmPayload, V2Eip155Exact};

/// Client for signing V2 EIP-155 exact scheme payments.
///
/// This client handles the creation and signing of ERC-3009 `transferWithAuthorization`
/// payments for EVM chains using the V2 protocol. Unlike V1, V2 uses CAIP-2 chain IDs
/// and embeds the accepted requirements directly in the payment payload.
///
/// # Type Parameters
///
/// - `S`: The signer type, which must implement [`SignerLike`](crate::v1_eip155_exact::client::SignerLike)
///
/// # Example
///
/// ```ignore
/// use x402_chain_eip155::V2Eip155ExactClient;
/// use alloy_signer_local::PrivateKeySigner;
///
/// let signer = PrivateKeySigner::random();
/// let client = V2Eip155ExactClient::new(signer);
/// ```
#[derive(Debug)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct V2Eip155ExactClient<S> {
    signer: S,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl<S> V2Eip155ExactClient<S> {
    /// Creates a new V2 EIP-155 exact scheme client with the given signer.
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
            .filter_map(|original_requirements_json| {
                let requirements = types::PaymentRequirements::try_from(original_requirements_json).ok()?;
                let chain_reference = Eip155ChainReference::try_from(&requirements.network).ok()?;
                let candidate = PaymentCandidate {
                    chain_id: requirements.network.clone(),
                    asset: requirements.asset.to_string(),
                    amount: requirements.amount.into(),
                    scheme: self.scheme().to_string(),
                    x402_version: self.x402_version(),
                    pay_to: requirements.pay_to.to_string(),
                    signer: Box::new(PayloadSigner {
                        resource_info: Some(payment_required.resource.clone()),
                        signer: self.signer.clone(),
                        chain_reference,
                        requirements,
                        requirements_json: original_requirements_json.clone(),
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
    resource_info: Option<ResourceInfo>,
    chain_reference: Eip155ChainReference,
    requirements: types::PaymentRequirements,
    requirements_json: OriginalJson,
}

#[async_trait]
impl<S> PaymentCandidateSigner for PayloadSigner<S>
where
    S: Sync + SignerLike,
{
    async fn sign_payment(&self) -> Result<String, X402Error> {
        let extra = match &self.requirements.extra {
            AssetTransferMethod::Eip3009 { name, version } => Some(PaymentRequirementsExtra {
                name: name.clone(),
                version: version.clone(),
            }),
            AssetTransferMethod::Permit2 => {
                todo!("Permit2 is not yet supported")
            }
        };

        let params = Eip3009SigningParams {
            chain_id: self.chain_reference.inner(),
            asset_address: self.requirements.asset.0,
            pay_to: self.requirements.pay_to.into(),
            amount: self.requirements.amount.into(),
            max_timeout_seconds: self.requirements.max_timeout_seconds,
            extra,
        };

        let evm_payload = sign_erc3009_authorization(&self.signer, &params).await?;

        // Build the payment payload
        let payload = types::PaymentPayload {
            x402_version: v2::X402Version2,
            accepted: self.requirements_json.clone(),
            resource: self.resource_info.clone(),
            payload: ExactEvmPayload::Eip3009(evm_payload), // FIXME Permit2
        };
        let json = serde_json::to_vec(&payload)?;
        let b64 = Base64Bytes::encode(&json);

        Ok(b64.to_string())
    }
}
