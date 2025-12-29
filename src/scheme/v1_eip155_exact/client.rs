use alloy_primitives::{Address, FixedBytes, Signature, U256};
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::{SolStruct, eip712_domain};
use async_trait::async_trait;
use rand::{Rng, rng};
use std::sync::Arc;

use crate::chain::ChainId;
use crate::chain::eip155::Eip155ChainReference;
use crate::proto::PaymentRequired;
use crate::proto::client::{PaymentCandidate, PaymentCandidateSigner, X402Error, X402SchemeClient};
use crate::proto::v1::X402Version1;
use crate::scheme::v1_eip155_exact::{
    ExactEvmPayload, ExactEvmPayloadAuthorization, ExactScheme, PaymentRequirementsExtra,
    TransferWithAuthorization, types,
};
use crate::scheme::{V1Eip155Exact, X402SchemeId};
use crate::timestamp::UnixTimestamp;
use crate::util::Base64Bytes;

#[derive(Debug)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct V1Eip155ExactClient<S> {
    signer: S,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl<S> V1Eip155ExactClient<S> {
    pub fn new(signer: S) -> Self {
        Self { signer }
    }
}

impl<S> X402SchemeId for V1Eip155ExactClient<S> {
    fn namespace(&self) -> &str {
        V1Eip155Exact.namespace()
    }

    fn scheme(&self) -> &str {
        V1Eip155Exact.scheme()
    }
}

impl<S> X402SchemeClient for V1Eip155ExactClient<S>
where
    S: SignerLike + Clone + Send + Sync + 'static,
{
    fn accept(&self, payment_required: &PaymentRequired) -> Vec<PaymentCandidate> {
        let payment_required = match payment_required {
            PaymentRequired::V1(payment_required) => payment_required,
            PaymentRequired::V2(_) => {
                return vec![];
            }
        };
        payment_required
            .accepts
            .iter()
            .filter_map(|v| {
                let requirements: types::PaymentRequirements = v.as_concrete()?;
                let chain_id = ChainId::from_network_name(&requirements.network)?;
                let chain_reference = Eip155ChainReference::try_from(chain_id.clone()).ok()?;
                let candidate = PaymentCandidate {
                    chain_id,
                    asset: requirements.asset.to_string(),
                    amount: requirements.max_amount_required,
                    scheme: self.scheme().to_string(),
                    x402_version: self.x402_version(),
                    pay_to: requirements.pay_to.to_string(),
                    signer: Box::new(PayloadSigner {
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

/// Shared EIP-712 signing parameters for ERC-3009 authorization.
/// Used by both v1 and v2 EIP-155 exact scheme clients.
#[derive(Debug, Clone)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct Eip3009SigningParams {
    /// The EIP-155 chain ID (numeric)
    pub chain_id: u64,
    /// The token contract address (verifying contract for EIP-712)
    pub asset_address: Address,
    /// The recipient address for the transfer
    pub pay_to: Address,
    /// The amount to transfer
    pub amount: U256,
    /// Maximum timeout in seconds for the authorization validity window
    pub max_timeout_seconds: u64,
    /// Optional EIP-712 domain name and version override
    pub extra: Option<PaymentRequirementsExtra>,
}

/// Signs an ERC-3009 TransferWithAuthorization using EIP-712.
///
/// This is the shared signing logic used by both v1 and v2 EIP-155 exact scheme clients.
/// It constructs the EIP-712 domain, builds the authorization struct with appropriate
/// timing parameters, and signs the resulting hash.
#[allow(dead_code)] // Public for consumption by downstream crates.
pub async fn sign_erc3009_authorization<S: SignerLike + Sync>(
    signer: &S,
    params: &Eip3009SigningParams,
) -> Result<ExactEvmPayload, X402Error> {
    // Extract name/version from extra, defaulting to empty strings
    let (name, version) = match &params.extra {
        None => ("".to_string(), "".to_string()),
        Some(extra) => (extra.name.clone(), extra.version.clone()),
    };

    // Build EIP-712 domain
    let domain = eip712_domain! {
        name: name,
        version: version,
        chain_id: params.chain_id,
        verifying_contract: params.asset_address,
    };

    // Build authorization with timing
    let now = UnixTimestamp::now();
    // valid_after should be in the past (10 minutes ago) to ensure the payment is immediately valid
    let valid_after_secs = now.as_secs().saturating_sub(10 * 60);
    let valid_after = UnixTimestamp::from_secs(valid_after_secs);
    let valid_before = now + params.max_timeout_seconds;
    let nonce: [u8; 32] = rng().random();
    let nonce = FixedBytes(nonce);

    let authorization = ExactEvmPayloadAuthorization {
        from: signer.address(),
        to: params.pay_to,
        value: params.amount,
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
    let signature = signer
        .sign_hash(&eip712_hash)
        .await
        .map_err(|e| X402Error::SigningError(format!("{e:?}")))?;

    Ok(ExactEvmPayload {
        signature: signature.as_bytes().into(),
        authorization,
    })
}

#[allow(dead_code)] // Public for consumption by downstream crates.
struct PayloadSigner<S> {
    signer: S,
    chain_reference: Eip155ChainReference,
    requirements: types::PaymentRequirements,
}

#[async_trait]
impl<S> PaymentCandidateSigner for PayloadSigner<S>
where
    S: SignerLike + Sync,
{
    async fn sign_payment(&self) -> Result<String, X402Error> {
        let params = Eip3009SigningParams {
            chain_id: self.chain_reference.inner(),
            asset_address: self.requirements.asset,
            pay_to: self.requirements.pay_to,
            amount: self.requirements.max_amount_required,
            max_timeout_seconds: self.requirements.max_timeout_seconds,
            extra: self.requirements.extra.clone(),
        };

        let evm_payload = sign_erc3009_authorization(&self.signer, &params).await?;

        // Build the payment payload
        let payload = types::PaymentPayload {
            x402_version: X402Version1,
            scheme: ExactScheme,
            network: self.requirements.network.clone(),
            payload: evm_payload,
        };
        let json = serde_json::to_vec(&payload)?;
        let b64 = Base64Bytes::encode(&json);

        Ok(b64.to_string())
    }
}

/// A trait that abstracts signing operations, allowing both owned signers and Arc-wrapped signers.
///
/// This is necessary because Alloy's `Signer` trait is not implemented for `Arc<T>`,
/// but users may want to share signers via `Arc` (especially when `PrivateKeySigner` doesn't implement `Clone`).
///
/// # Example
///
/// ```ignore
/// use std::sync::Arc;
/// use alloy_signer_local::PrivateKeySigner;
/// use x402_rs::scheme::v1_eip155_exact::SignerLike;
///
/// let signer: PrivateKeySigner = ...;
/// let signer = Arc::new(signer);
/// // Now you can use `signer` anywhere `SignerLike` is expected
/// ```
#[async_trait]
pub trait SignerLike {
    /// Returns the address of the signer.
    fn address(&self) -> Address;

    /// Signs the given hash.
    async fn sign_hash(&self, hash: &FixedBytes<32>) -> Result<Signature, alloy_signer::Error>;
}

#[async_trait]
impl SignerLike for PrivateKeySigner {
    fn address(&self) -> Address {
        PrivateKeySigner::address(self)
    }

    async fn sign_hash(&self, hash: &FixedBytes<32>) -> Result<Signature, alloy_signer::Error> {
        alloy_signer::Signer::sign_hash(self, hash).await
    }
}

#[async_trait]
impl SignerLike for Arc<PrivateKeySigner> {
    fn address(&self) -> Address {
        PrivateKeySigner::address(self.as_ref())
    }

    async fn sign_hash(&self, hash: &FixedBytes<32>) -> Result<Signature, alloy_signer::Error> {
        alloy_signer::Signer::sign_hash(self.as_ref(), hash).await
    }
}
