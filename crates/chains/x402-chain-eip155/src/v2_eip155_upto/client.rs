//! Client-side payment signing for the V2 EIP-155 "upto" scheme.
//!
//! This module provides [`V2Eip155UptoClient`] for signing Permit2-based "upto"
//! payments on EVM chains using the V2 protocol.
//!
//! # Usage
//!
//! ```ignore
//! use x402_chain_eip155::v2_eip155_upto::client::V2Eip155UptoClient;
//! use alloy_signer_local::PrivateKeySigner;
//!
//! let signer = PrivateKeySigner::random();
//! let client = V2Eip155UptoClient::new(signer);
//! ```

use alloy_primitives::{Address, U256};
use alloy_sol_types::{SolStruct, eip712_domain};
use async_trait::async_trait;
use rand::{Rng, rng};
use x402_types::proto::v2::ResourceInfo;
use x402_types::proto::{OriginalJson, PaymentRequired, v2};
use x402_types::scheme::X402SchemeId;
use x402_types::scheme::client::{
    PaymentCandidate, PaymentCandidateSigner, X402Error, X402SchemeClient,
};
use x402_types::timestamp::UnixTimestamp;
use x402_types::util::Base64Bytes;

use crate::chain::{Eip155ChainReference};
use crate::v1_eip155_exact::client::{SignerLike};
use crate::v2_eip155_upto::V2Eip155Upto;
use crate::v2_eip155_upto::types;
use crate::v2_eip155_upto::types::{
    ISignatureTransfer, PermitWitnessTransferFrom, x402BasePermit2Proxy,
};

/// Parameters for signing a Permit2 upto authorization.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct Permit2UptoSigningParams {
    /// The EIP-155 chain ID (numeric)
    pub chain_id: u64,
    /// The token contract address to transfer
    pub asset_address: Address,
    /// The recipient address for the transfer
    pub pay_to: Address,
    /// The maximum amount that can be transferred
    pub max_amount: U256,
    /// Maximum timeout in seconds for the authorization validity window
    pub max_timeout_seconds: u64,
    /// Optional extra data to include in the witness
    pub extra: Option<Vec<u8>>,
}

/// Signs a Permit2 PermitWitnessTransferFrom for the upto scheme using EIP-712.
///
/// This constructs the EIP-712 domain for Permit2, builds the authorization struct
/// with appropriate timing parameters, and signs the resulting hash.
///
/// The `max_amount` represents the maximum that can be charged at settlement time.
#[allow(dead_code)] // Public for consumption by downstream crates.
pub async fn sign_permit2_upto_authorization<S: SignerLike + Sync>(
    signer: &S,
    params: &Permit2UptoSigningParams,
) -> Result<types::Permit2Payload, X402Error> {
    // Build EIP-712 domain for Permit2
    let domain = eip712_domain! {
        name: "Permit2",
        chain_id: params.chain_id,
        verifying_contract: types::PERMIT2_ADDRESS,
    };

    // Build authorization with timing
    let now = UnixTimestamp::now();
    // valid_after should be in the past (10 minutes ago) to ensure the payment is immediately valid
    let valid_after_secs = now.as_secs().saturating_sub(10 * 60);
    let valid_after = UnixTimestamp::from_secs(valid_after_secs);
    let deadline = now + params.max_timeout_seconds;

    // Generate a random nonce
    let nonce: [u8; 32] = rng().random();
    let nonce = U256::from_be_bytes(nonce);

    // Build the PermitWitnessTransferFrom struct for signing
    let permit_witness_transfer_from = PermitWitnessTransferFrom {
        permitted: ISignatureTransfer::TokenPermissions {
            token: params.asset_address,
            amount: params.max_amount,
        },
        spender: types::UPTO_PERMIT2_PROXY_ADDRESS,
        nonce,
        deadline: U256::from(deadline.as_secs()),
        witness: x402BasePermit2Proxy::Witness {
            to: params.pay_to,
            validAfter: U256::from(valid_after.as_secs()),
            extra: params.extra.clone().unwrap_or_default().into(),
        },
    };

    let eip712_hash = permit_witness_transfer_from.eip712_signing_hash(&domain);
    let signature = signer
        .sign_hash(&eip712_hash)
        .await
        .map_err(|e| X402Error::SigningError(format!("{e:?}")))?;

    // Build the Permit2Authorization for the payload
    let authorization = types::Permit2Authorization {
        deadline,
        from: signer.address().into(),
        nonce,
        permitted: types::Permit2AuthorizationPermitted {
            amount: params.max_amount,
            token: params.asset_address.into(),
        },
        spender: types::UPTO_PERMIT2_PROXY_ADDRESS.into(),
        witness: types::Permit2Witness {
            extra: permit_witness_transfer_from.witness.extra.clone(),
            to: params.pay_to.into(),
            valid_after,
        },
    };

    Ok(types::Permit2Payload {
        permit_2_authorization: authorization,
        signature: signature.as_bytes().into(),
    })
}

/// Client for signing V2 EIP-155 upto scheme payments.
///
/// This client handles the creation and signing of Permit2-based "upto" payments
/// for EVM chains using the V2 protocol. The client authorizes a maximum amount,
/// and the server settles for the actual amount used at settlement time.
///
/// # Type Parameters
///
/// - `S`: The signer type, which must implement [`SignerLike`](crate::v1_eip155_exact::client::SignerLike)
///
/// # Example
///
/// ```ignore
/// use x402_chain_eip155::V2Eip155UptoClient;
/// use alloy_signer_local::PrivateKeySigner;
///
/// let signer = PrivateKeySigner::random();
/// let client = V2Eip155UptoClient::new(signer);
/// ```
#[derive(Debug)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct V2Eip155UptoClient<S> {
    signer: S,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl<S> V2Eip155UptoClient<S> {
    /// Creates a new V2 EIP-155 upto scheme client with the given signer.
    pub fn new(signer: S) -> Self {
        Self { signer }
    }
}

impl<S> X402SchemeId for V2Eip155UptoClient<S> {
    fn namespace(&self) -> &str {
        V2Eip155Upto.namespace()
    }

    fn scheme(&self) -> &str {
        V2Eip155Upto.scheme()
    }
}

impl<S> X402SchemeClient for V2Eip155UptoClient<S>
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
                let requirements =
                    types::PaymentRequirements::try_from(original_requirements_json).ok()?;
                // Verify this is an "upto" scheme
                if requirements.scheme != types::UptoScheme {
                    return None;
                }
                let chain_reference = Eip155ChainReference::try_from(&requirements.network).ok()?;
                let candidate = PaymentCandidate {
                    chain_id: requirements.network.clone(),
                    asset: requirements.asset.to_string(),
                    amount: requirements.amount,
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
        // Build the payment payload for Permit2 upto
        let params = Permit2UptoSigningParams {
            chain_id: self.chain_reference.inner(),
            asset_address: self.requirements.asset.0,
            pay_to: self.requirements.pay_to.into(),
            max_amount: self.requirements.amount,
            max_timeout_seconds: self.requirements.max_timeout_seconds,
            extra: None,
        };

        let permit2_payload = sign_permit2_upto_authorization(&self.signer, &params).await?;

        let payload = v2::PaymentPayload {
            x402_version: v2::X402Version2,
            accepted: self.requirements_json.clone(),
            resource: self.resource_info.clone(),
            payload: permit2_payload,
        };

        let json = serde_json::to_vec(&payload)?;
        let b64 = Base64Bytes::encode(&json);

        Ok(b64.to_string())
    }
}
