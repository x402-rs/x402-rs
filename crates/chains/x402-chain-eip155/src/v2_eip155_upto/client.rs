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
use alloy_provider::ProviderBuilder;
use alloy_sol_types::{SolStruct, eip712_domain};
use async_trait::async_trait;
use rand::{RngExt, rng};
use serde::{Deserialize, Serialize};
use url::Url;
use x402_types::proto::v2::{ExtensionsJson, ResourceInfo};
use x402_types::proto::{OriginalJson, PaymentRequired, v2};
use x402_types::scheme::client::{
    PaymentCandidate, PaymentCandidateSigner, X402Error, X402SchemeClient,
};
use x402_types::scheme::{ExtensionKey, X402SchemeId};
use x402_types::timestamp::UnixTimestamp;
use x402_types::util::Base64Bytes;

use crate::chain::permit2::{
    PERMIT2_ADDRESS, Permit2Authorization, Permit2AuthorizationPermitted,
    UPTO_PERMIT2_PROXY_ADDRESS, UptoPermit2Payload, UptoPermit2Witness,
};
use crate::chain::{ChecksummedAddress, EOASignature, Eip155ChainReference};
use crate::eip2612_gas_sponsoring::{
    Eip2612GasSponsoring, Eip2612GasSponsoringInfo, Eip2612GasSponsoringServer, Permit,
};
use crate::v1_eip155_exact::client::SignerLike;
use crate::v2_eip155_upto::types::{ISignatureTransfer, PermitWitnessTransferFrom};
use crate::v2_eip155_upto::{IERC20Permit, UptoSupportedExtra, V2Eip155Upto};
use crate::v2_eip155_upto::{types, x402UptoPermit2Proxy};

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
    /// The facilitator address authorized to settle this payment
    pub facilitator: Address,
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
) -> Result<UptoPermit2Payload, X402Error> {
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
            amount: params.max_amount,
        },
        spender: UPTO_PERMIT2_PROXY_ADDRESS,
        nonce,
        deadline: U256::from(deadline.as_secs()),
        witness: x402UptoPermit2Proxy::Witness {
            to: params.pay_to,
            facilitator: params.facilitator,
            validAfter: U256::from(valid_after.as_secs()),
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
            amount: params.max_amount,
            token: params.asset_address.into(),
        },
        spender: UPTO_PERMIT2_PROXY_ADDRESS.into(),
        witness: UptoPermit2Witness {
            to: params.pay_to.into(),
            facilitator: params.facilitator.into(),
            valid_after,
        },
    };

    Ok(UptoPermit2Payload {
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
pub struct V2Eip155UptoClient<S, P> {
    signer: S,
    provider: P,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl<S> V2Eip155UptoClient<S, ()> {
    /// Creates a new V2 EIP-155 upto scheme client with the given signer.
    pub fn new(signer: S) -> Self {
        Self {
            signer,
            provider: (),
        }
    }
}

impl<S, P> V2Eip155UptoClient<S, P> {
    // FIXME Doc comments
    pub fn with_provider<P2>(self, provider: P2) -> V2Eip155UptoClient<S, P2> {
        V2Eip155UptoClient {
            signer: self.signer,
            provider,
        }
    }
}

impl<S, P> X402SchemeId for V2Eip155UptoClient<S, P> {
    fn namespace(&self) -> &str {
        V2Eip155Upto.namespace()
    }

    fn scheme(&self) -> &str {
        V2Eip155Upto.scheme()
    }
}

impl<S, P> X402SchemeClient for V2Eip155UptoClient<S, P>
where
    S: SignerLike + Clone + Send + Sync + 'static,
    P: Clone + DoRead2612Nonce + Send + Sync + 'static,
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
                        resource_info: payment_required.resource.clone(),
                        signer: self.signer.clone(),
                        provider: self.provider.clone(),
                        chain_reference,
                        requirements,
                        extensions: payment_required.extensions.clone(),
                        requirements_json: original_requirements_json.clone(),
                    }),
                };
                Some(candidate)
            })
            .collect::<Vec<_>>()
    }
}

#[allow(dead_code)] // Public for consumption by downstream crates.
struct PayloadSigner<S, P> {
    signer: S,
    provider: P,
    resource_info: Option<ResourceInfo>,
    extensions: Option<ExtensionsJson>,
    chain_reference: Eip155ChainReference,
    requirements: types::PaymentRequirements,
    requirements_json: OriginalJson,
}

#[async_trait]
impl<S, P> PaymentCandidateSigner for PayloadSigner<S, P>
where
    S: Sync + SignerLike,
    P: DoRead2612Nonce + Send + Sync + 'static,
{
    async fn sign_payment(&self) -> Result<String, X402Error> {
        println!("sign_payment.0");
        // The server must provide the facilitator address via requirements.extra.facilitatorAddress
        let facilitator_address = self
            .requirements
            .extra
            .as_ref()
            .and_then(|v| serde_json::from_value::<UptoSupportedExtra<Address>>(v.clone()).ok())
            .map(|extra| extra.facilitator_address)
            .ok_or(X402Error::SigningError(
                "upto scheme requires facilitatorAddress in payment requirements extra".to_string(),
            ))?;

        let params = Permit2UptoSigningParams {
            chain_id: self.chain_reference.inner(),
            asset_address: self.requirements.asset.0,
            pay_to: self.requirements.pay_to.into(),
            max_amount: self.requirements.amount,
            max_timeout_seconds: self.requirements.max_timeout_seconds,
            facilitator: facilitator_address,
        };

        println!("sign_payment.1");
        let permit2_payload = sign_permit2_upto_authorization(&self.signer, &params).await?;
        println!("sign_payment.2");
        println!("extensions {:?}", self.extensions);
        let eip2612_gas_sponsoring = self
            .extensions
            .as_ref()
            .and_then(|extensions| extensions.get::<Eip2612GasSponsoringServer>());
        let mut extension_map = serde_json::Map::new(); // FIXME this is a bit ugly
        if let Some(eip2612_gas_sponsoring) = eip2612_gas_sponsoring {
            /// Token name and version for permit signature
            #[derive(Debug, Clone, Serialize, Deserialize)]
            struct TokenDomain {
                pub name: String,
                pub version: String,
            }

            println!("eip2612_gas_sponsoring {:?}", eip2612_gas_sponsoring);
            let token_domain = self
                .requirements
                .extra
                .as_ref()
                .and_then(|v| serde_json::from_value::<TokenDomain>(v.clone()).ok())
                .ok_or(X402Error::SigningError(
                    "extra should contain token name and version for eip2612GasSponsoring"
                        .to_string(),
                ))?;
            let owner = self.signer.address();
            let deadline = permit2_payload.permit_2_authorization.deadline;
            let token_contract = self.requirements.asset;
            let nonce = self
                .provider
                .read_2612_nonce(self.requirements.asset.into(), owner)
                .await?;
            let value = self.requirements.amount;

            let domain = eip712_domain! {
                name: token_domain.name,
                version: token_domain.version,
                chain_id: self.chain_reference.inner(),
                verifying_contract: token_contract.into(),
            };
            let permit = Permit {
                owner,
                spender: PERMIT2_ADDRESS,
                value,
                nonce,
                deadline: U256::from(deadline.as_secs()),
            };
            let signature = self
                .signer
                .sign_hash(&permit.eip712_signing_hash(&domain))
                .await
                .map_err(|e| X402Error::SigningError(format!("{e:?}")))?;

            let info = Eip2612GasSponsoringInfo {
                from: ChecksummedAddress::from(owner),
                asset: self.requirements.asset,
                spender: ChecksummedAddress::from(PERMIT2_ADDRESS),
                amount: value,
                nonce,
                deadline,
                signature: EOASignature::from(signature),
                version: eip2612_gas_sponsoring.info.version,
            };
            let eip2612_gas_sponsoring = Eip2612GasSponsoring { info };

            extension_map.insert(
                Eip2612GasSponsoring::EXTENSION_KEY.to_string(),
                serde_json::to_value(eip2612_gas_sponsoring)?,
            );
            // TODO Check against json schema??
            // TODO THink if it makes sense to have extension/key/whatever separation differently

            // FIXME CONTINUE HERE sign the payment
        }

        let extensions = ExtensionsJson::from_iter(extension_map)?;

        let payload = v2::PaymentPayload {
            x402_version: v2::X402Version2,
            accepted: self.requirements_json.clone(),
            resource: self.resource_info.clone(),
            payload: permit2_payload,
            extensions: Some(extensions),
        };

        let json = serde_json::to_vec(&payload)?;
        let b64 = Base64Bytes::encode(&json);

        Ok(b64.to_string())
    }
}

// FIXME Docs
pub trait DoRead2612Nonce {
    fn read_2612_nonce(
        &self,
        asset: Address,
        owner: Address,
    ) -> impl Future<Output = Result<U256, X402Error>> + Send;
}

impl DoRead2612Nonce for Url {
    async fn read_2612_nonce(&self, asset: Address, owner: Address) -> Result<U256, X402Error> {
        let provider = ProviderBuilder::new().connect_http(self.clone());
        let token = IERC20Permit::new(asset, provider);
        let nonce =
            token.nonces(owner).call().await.map_err(|e| {
                X402Error::SigningError(format!("failed to get permit nonce {e:?}"))
            })?;
        Ok(nonce)
    }
}
