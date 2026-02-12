//! Client-side payment signing for the V2 EIP-155 "exact" scheme.
//!
//! This module provides [`V2Eip155ExactClient`] for signing ERC-3009
//! `transferWithAuthorization` payments and Permit2 transfers on EVM chains
//! using the V2 protocol.
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

use crate::chain::permit2::{
    EXACT_PERMIT2_PROXY_ADDRESS, PERMIT2_ADDRESS, Permit2Authorization,
    Permit2AuthorizationPermitted, Permit2Payload, Permit2Witness,
};
use crate::chain::{AssetTransferMethod, Eip155ChainReference};
use crate::v1_eip155_exact::PaymentRequirementsExtra;
use crate::v1_eip155_exact::client::{
    Eip3009SigningParams, SignerLike, sign_erc3009_authorization,
};
use crate::v2_eip155_exact::V2Eip155Exact;
use crate::v2_eip155_exact::types;
use crate::v2_eip155_exact::types::{
    ExactEvmPayload, ISignatureTransfer, PermitWitnessTransferFrom, x402BasePermit2Proxy,
};

/// Parameters for signing a Permit2 authorization.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct Permit2SigningParams {
    /// The EIP-155 chain ID (numeric)
    pub chain_id: u64,
    /// The token contract address to transfer
    pub asset_address: Address,
    /// The recipient address for the transfer
    pub pay_to: Address,
    /// The amount to transfer
    pub amount: U256,
    /// Maximum timeout in seconds for the authorization validity window
    pub max_timeout_seconds: u64,
    /// Optional extra data to include in the witness
    pub extra: Option<Vec<u8>>,
}

/// Signs a Permit2 PermitWitnessTransferFrom using EIP-712.
///
/// This constructs the EIP-712 domain for Permit2, builds the authorization struct
/// with appropriate timing parameters, and signs the resulting hash.
#[allow(dead_code)] // Public for consumption by downstream crates.
pub async fn sign_permit2_authorization<S: SignerLike + Sync>(
    signer: &S,
    params: &Permit2SigningParams,
) -> Result<Permit2Payload, X402Error> {
    // Build EIP-712 domain for Permit2
    let domain = eip712_domain! {
        name: "Permit2",
        chain_id: params.chain_id,
        verifying_contract: PERMIT2_ADDRESS,
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
            amount: params.amount,
        },
        spender: EXACT_PERMIT2_PROXY_ADDRESS,
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
    let authorization = Permit2Authorization {
        deadline,
        from: signer.address().into(),
        nonce,
        permitted: Permit2AuthorizationPermitted {
            amount: params.amount,
            token: params.asset_address.into(),
        },
        spender: EXACT_PERMIT2_PROXY_ADDRESS.into(),
        witness: Permit2Witness {
            extra: permit_witness_transfer_from.witness.extra.clone(),
            to: params.pay_to.into(),
            valid_after,
        },
    };

    Ok(Permit2Payload {
        permit_2_authorization: authorization,
        signature: signature.as_bytes().into(),
    })
}

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
                let requirements =
                    types::PaymentRequirements::try_from(original_requirements_json).ok()?;
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
        // Build the payment payload based on the asset transfer method
        let payload = match &self.requirements.extra {
            AssetTransferMethod::Eip3009 { name, version } => {
                let extra = Some(PaymentRequirementsExtra {
                    name: name.clone(),
                    version: version.clone(),
                });

                let params = Eip3009SigningParams {
                    chain_id: self.chain_reference.inner(),
                    asset_address: self.requirements.asset.0,
                    pay_to: self.requirements.pay_to.into(),
                    amount: self.requirements.amount,
                    max_timeout_seconds: self.requirements.max_timeout_seconds,
                    extra,
                };

                let evm_payload = sign_erc3009_authorization(&self.signer, &params).await?;
                v2::PaymentPayload {
                    x402_version: v2::X402Version2,
                    accepted: self.requirements_json.clone(),
                    resource: self.resource_info.clone(),
                    payload: ExactEvmPayload::Eip3009(evm_payload),
                }
            }
            AssetTransferMethod::Permit2 => {
                let params = Permit2SigningParams {
                    chain_id: self.chain_reference.inner(),
                    asset_address: self.requirements.asset.0,
                    pay_to: self.requirements.pay_to.into(),
                    amount: self.requirements.amount,
                    max_timeout_seconds: self.requirements.max_timeout_seconds,
                    extra: None,
                };

                let permit2_payload = sign_permit2_authorization(&self.signer, &params).await?;
                v2::PaymentPayload {
                    x402_version: v2::X402Version2,
                    accepted: self.requirements_json.clone(),
                    resource: self.resource_info.clone(),
                    payload: ExactEvmPayload::Permit2(permit2_payload),
                }
            }
        };

        let json = serde_json::to_vec(&payload)?;
        let b64 = Base64Bytes::encode(&json);

        Ok(b64.to_string())
    }
}
