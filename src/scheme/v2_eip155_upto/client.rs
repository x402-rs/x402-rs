//! Client-side payment signing for the V2 EIP-155 "upto" scheme.
//!
//! This module provides [`V2Eip155UptoClient`] for signing EIP-2612 permit
//! payments on EVM chains using the V2 protocol.
//!
//! # Usage
//!
//! ```ignore
//! use x402_rs::scheme::v2_eip155_upto::client::V2Eip155UptoClient;
//! use alloy_signer_local::PrivateKeySigner;
//!
//! let signer = PrivateKeySigner::random();
//! let client = V2Eip155UptoClient::new(signer);
//! ```

use crate::chain::eip155::Eip155ChainReference;
use crate::proto::v2::ResourceInfo;
use crate::proto::{PaymentRequired, v2};
use crate::scheme::X402SchemeId;
use crate::scheme::client::{
    PaymentCandidate, PaymentCandidateSigner, X402Error, X402SchemeClient,
};
use crate::scheme::v1_eip155_exact::client::SignerLike;
use crate::scheme::v2_eip155_upto::V2Eip155Upto;
use crate::scheme::v2_eip155_upto::types;
use crate::timestamp::UnixTimestamp;
use crate::util::Base64Bytes;
use alloy_primitives::{Address, U256};
use alloy_provider::ProviderBuilder;
use alloy_sol_types::{eip712_domain, sol, SolStruct};
use async_trait::async_trait;

// EIP-2612 Permit struct for signing
sol! {
    #[derive(Debug)]
    struct Permit {
        address owner;
        address spender;
        uint256 value;
        uint256 nonce;
        uint256 deadline;
    }
}

#[derive(Debug)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct V2Eip155UptoClient<S> {
    signer: S,
    /// Facilitator signer address (required for permit signing)
    facilitator_signer: Address,
    /// Optional RPC URL for fetching token nonces (defaults to env var or public RPC)
    rpc_url: Option<String>,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl<S> V2Eip155UptoClient<S> {
    pub fn new(signer: S, facilitator_signer: Address) -> Self {
        Self {
            signer,
            facilitator_signer,
            rpc_url: None,
        }
    }

    pub fn with_rpc_url(mut self, url: String) -> Self {
        self.rpc_url = Some(url);
        self
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
            .filter_map(|v| {
                let requirements: types::PaymentRequirements = v.as_concrete()?;
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
                        facilitator_signer: self.facilitator_signer,
                        rpc_url: self.rpc_url.clone(),
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
    facilitator_signer: Address,
    rpc_url: Option<String>,
}

/// Fetch current nonce from token contract
async fn fetch_token_nonce(
    rpc_url: &str,
    token_address: Address,
    owner: Address,
) -> Result<U256, X402Error> {
    // Create provider
    let url: url::Url = rpc_url.parse().map_err(|e| {
        X402Error::SigningError(format!("Invalid RPC URL: {}", e))
    })?;
    let provider = ProviderBuilder::new()
        .connect_http(url);

    // Call nonces function on token contract
    // Using IEIP3009 interface which includes nonces()
    use crate::scheme::v1_eip155_exact::IEIP3009;
    let contract = IEIP3009::new(token_address, &provider);
    let nonce_result: U256 = contract
        .nonces(owner)
        .call()
        .await
        .map_err(|e| X402Error::SigningError(format!("Failed to fetch nonce: {}", e)))?;

    Ok(nonce_result)
}

#[async_trait]
impl<S> PaymentCandidateSigner for PayloadSigner<S>
where
    S: Sync + SignerLike,
{
    async fn sign_payment(&self) -> Result<String, X402Error> {
        // Get EIP-712 domain info
        let extra = self.requirements.extra.as_ref().ok_or_else(|| {
            X402Error::SigningError("Missing EIP-712 domain info (name/version) in requirements.extra".to_string())
        })?;

        let chain_id = self.chain_reference.inner();
        let asset_address = self.requirements.asset.0;
        let owner = self.signer.address();
        let amount = self.requirements.amount.0;
        let facilitator_address = self.facilitator_signer;

        // Determine permit cap
        // Use max_amount_required if specified, otherwise use a reasonable multiple of amount
        let cap = if let Some(max_amount) = extra.max_amount_required {
            max_amount
        } else {
            // Default to 10x the required amount for batched payments
            amount * U256::from(10)
        };

        // Fetch current nonce from token contract
        // RPC URL must be provided via with_rpc_url()
        let rpc_url = self.rpc_url.as_deref().ok_or_else(|| {
            X402Error::SigningError(
                "RPC URL required for fetching token nonce. Call with_rpc_url() on V2Eip155UptoClient".to_string()
            )
        })?;
        
        let nonce = fetch_token_nonce(rpc_url, asset_address, owner).await?;

        // Calculate deadline (now + max_timeout_seconds)
        let now = UnixTimestamp::now();
        let deadline = now.as_secs() + self.requirements.max_timeout_seconds;
        let deadline_u256 = U256::from(deadline);

        // Build EIP-712 domain
        let domain = eip712_domain! {
            name: extra.name.clone(),
            version: extra.version.clone(),
            chain_id: chain_id,
            verifying_contract: asset_address,
        };

        // Build permit struct
        let permit = Permit {
            owner,
            spender: facilitator_address,
            value: cap,
            nonce,
            deadline: deadline_u256,
        };

        // Compute EIP-712 hash
        let eip712_hash = permit.eip712_signing_hash(&domain);

        // Sign the hash
        let signature = self
            .signer
            .sign_hash(&eip712_hash)
            .await
            .map_err(|e| X402Error::SigningError(format!("{e:?}")))?;

        // Build authorization
        let authorization = types::UptoEvmAuthorization {
            from: owner,
            to: facilitator_address,
            value: cap,
            nonce,
            valid_before: deadline_u256,
        };

        // Build payload
        let payload = types::PaymentPayload {
            x402_version: v2::X402Version2,
            accepted: self.requirements.clone(),
            resource: self.resource_info.clone(),
            payload: types::UptoEvmPayload {
                signature: signature.as_bytes().into(),
                authorization,
            },
        };

        let json = serde_json::to_vec(&payload)?;
        let b64 = Base64Bytes::encode(&json);

        Ok(b64.to_string())
    }
}
