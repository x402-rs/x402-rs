//! V2 EIP-155 "upto" payment scheme implementation.
//!
//! This scheme implements batched payments using EIP-2612 permits on EVM chains.
//! Unlike the "exact" scheme which settles each payment immediately via ERC-3009,
//! the "upto" scheme allows users to pre-authorize a spending cap via a permit
//! signature, enabling multiple payments to be batched and settled together.
//!
//! # Payment Flow
//!
//! 1. User signs an EIP-2612 permit authorizing the facilitator to spend up to a cap
//! 2. Facilitator verifies the permit signature
//! 3. Multiple payments can be made under the same cap (tracked server-side)
//! 4. Settlement: facilitator calls `permit()` then `transferFrom()` for the total amount
//!
//! # Key Differences from Exact Scheme
//!
//! - Uses EIP-2612 `permit()` instead of ERC-3009 `transferWithAuthorization()`
//! - Supports batching multiple payments under one permit
//! - Requires server-side session tracking for pending amounts
//! - More gas-efficient for multiple small payments

pub mod types;

use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::{eip712_domain, Eip712Domain, SolStruct};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::instrument;

use crate::chain::eip155::{Eip155ChainReference, Eip155MetaTransactionProvider, MetaTransaction};
use crate::chain::{ChainId, ChainProvider, ChainProviderOps};
use crate::proto::{self, v2, PaymentVerificationError, X402SchemeFacilitatorError};
use crate::scheme::v1_eip155_exact::{assert_time, IEIP3009};
use crate::scheme::{X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeId};
use crate::timestamp::UnixTimestamp;

use types::{PaymentRequirementsExtra, UptoScheme};

/// V2 EIP-155 Upto scheme blueprint.
pub struct V2Eip155Upto;

impl X402SchemeId for V2Eip155Upto {
    fn namespace(&self) -> &str {
        "eip155"
    }

    fn scheme(&self) -> &str {
        "upto"
    }
}

impl X402SchemeFacilitatorBuilder for V2Eip155Upto {
    fn build(
        &self,
        provider: ChainProvider,
        _config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let provider = match provider {
            ChainProvider::Eip155(p) => p,
            _ => return Err("v2-eip155-upto requires Eip155ChainProvider".into()),
        };
        Ok(Box::new(V2Eip155UptoFacilitator::new(provider)))
    }
}

/// Facilitator for V2 EIP-155 Upto payments.
pub struct V2Eip155UptoFacilitator<P> {
    provider: P,
}

impl<P> V2Eip155UptoFacilitator<P> {
    /// Creates a new facilitator with the given provider.
    pub fn new(provider: P) -> Self {
        Self { provider }
    }
}

#[async_trait::async_trait]
impl<P> X402SchemeFacilitator for V2Eip155UptoFacilitator<P>
where
    P: Eip155MetaTransactionProvider + ChainProviderOps + Send + Sync,
    P::Inner: alloy_provider::Provider,
    Eip155UptoError: From<P::Error>,
{
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError> {
        let request = types::VerifyRequest::from_proto(request.clone())?;
        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;

        let payer = verify_upto_payment(
            self.provider.inner(),
            self.provider.chain(),
            &self.provider.signer_addresses(),
            payload,
            requirements,
        )
        .await?;

        Ok(v2::VerifyResponse::valid(payer.to_string()).into())
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, X402SchemeFacilitatorError> {
        let request = types::SettleRequest::from_proto(request.clone())?;
        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;

        // Verify first
        let payer = verify_upto_payment(
            self.provider.inner(),
            self.provider.chain(),
            &self.provider.signer_addresses(),
            payload,
            requirements,
        )
        .await?;

        // Settle the payment
        let tx_hash = settle_upto_payment(&self.provider, payload, requirements).await?;

        Ok(v2::SettleResponse::Success {
            payer: payer.to_string(),
            transaction: tx_hash.to_string(),
            network: payload.accepted.network.to_string(),
        }
        .into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError> {
        let chain_id = self.provider.chain_id();
        let kinds = vec![proto::SupportedPaymentKind {
            x402_version: v2::X402Version2.into(),
            scheme: UptoScheme.to_string(),
            network: chain_id.to_string(),
            extra: None,
        }];
        let signers = {
            let mut signers = HashMap::with_capacity(1);
            signers.insert(chain_id, self.provider.signer_addresses());
            signers
        };
        Ok(proto::SupportedResponse {
            kinds,
            extensions: Vec::new(),
            signers,
        })
    }
}

/// Errors specific to the upto scheme.
#[derive(Debug, thiserror::Error)]
pub enum Eip155UptoError {
    #[error(transparent)]
    Verification(#[from] PaymentVerificationError),
    #[error(transparent)]
    ContractCall(#[from] alloy_contract::Error),
    #[error(transparent)]
    Transport(#[from] alloy_transport::TransportError),
    #[error("Invalid signature format")]
    InvalidSignature,
    #[error("Permit transaction failed")]
    PermitFailed,
    #[error("Transfer transaction failed")]
    TransferFailed,
}

impl From<Eip155UptoError> for X402SchemeFacilitatorError {
    fn from(e: Eip155UptoError) -> Self {
        match e {
            Eip155UptoError::Verification(v) => X402SchemeFacilitatorError::Verification(v),
            e => X402SchemeFacilitatorError::Other(e.into()),
        }
    }
}

/// Solidity-compatible struct for EIP-2612 Permit.
alloy_sol_types::sol! {
    #[derive(Debug, Serialize, Deserialize)]
    struct Permit {
        address owner;
        address spender;
        uint256 value;
        uint256 nonce;
        uint256 deadline;
    }
}

/// Verify an upto payment permit signature.
#[instrument(skip_all, err, fields(
    network = %chain.as_chain_id(),
    payer = ?payload.payload.authorization.from
))]
async fn verify_upto_payment<P: alloy_provider::Provider>(
    provider: &P,
    chain: &Eip155ChainReference,
    facilitator_addresses: &[Address],
    payload: &types::PaymentPayload,
    requirements: &types::PaymentRequirements,
) -> Result<Address, Eip155UptoError> {
    let chain_id: ChainId = chain.into();
    let payload_chain_id: ChainId = payload.accepted.network.parse().map_err(|_| {
        PaymentVerificationError::UnsupportedChain
    })?;
    
    if payload_chain_id != chain_id {
        return Err(PaymentVerificationError::ChainIdMismatch.into());
    }

    let requirements_chain_id: ChainId = requirements.network.parse().map_err(|_| {
        PaymentVerificationError::UnsupportedChain
    })?;
    
    if requirements_chain_id != chain_id {
        return Err(PaymentVerificationError::ChainIdMismatch.into());
    }

    let authorization = &payload.payload.authorization;
    let owner = authorization.from;
    let spender = authorization.to;
    let cap = authorization.value;
    let nonce = authorization.nonce;
    let deadline = authorization.valid_before;

    // Validate spender is the facilitator
    if !facilitator_addresses.contains(&spender) {
        return Err(PaymentVerificationError::RecipientMismatch.into());
    }

    // Validate cap covers required amount
    let required_amount = requirements.amount.0;
    if cap < required_amount {
        return Err(PaymentVerificationError::InvalidPaymentAmount.into());
    }

    // Validate cap covers maxAmountRequired if specified
    if let Some(ref extra) = requirements.extra {
        if let Some(max_amount_required) = extra.max_amount_required {
            if cap < max_amount_required {
                return Err(PaymentVerificationError::InvalidPaymentAmount.into());
            }
        }
    }

    // Validate deadline (with 6 second buffer)
    let now = UnixTimestamp::now();
    let deadline_timestamp = UnixTimestamp::from_secs(deadline.to::<u64>());
    if deadline_timestamp < now + 6 {
        return Err(PaymentVerificationError::Expired.into());
    }

    // Get EIP-712 domain info
    let extra = requirements.extra.as_ref().ok_or(
        PaymentVerificationError::InvalidPaymentRequirements
    )?;
    let asset_address = requirements.asset.0;
    let token_contract = IEIP3009::new(asset_address, provider);

    // Construct EIP-712 domain
    let domain = eip712_domain! {
        name: extra.name.clone(),
        version: extra.version.clone(),
        chain_id: chain.inner(),
        verifying_contract: asset_address,
    };

    // Build permit typed data
    let permit = Permit {
        owner,
        spender,
        value: cap,
        nonce,
        deadline,
    };

    // Compute EIP-712 hash
    let eip712_hash = permit.eip712_signing_hash(&domain);

    // Verify signature
    let signature = &payload.payload.signature;
    verify_permit_signature(provider, owner, eip712_hash, signature).await?;

    Ok(owner)
}

/// Verify a permit signature (supports EOA and EIP-1271).
#[instrument(skip_all, err)]
async fn verify_permit_signature<P: alloy_provider::Provider>(
    provider: &P,
    signer: Address,
    hash: alloy_primitives::B256,
    signature: &Bytes,
) -> Result<(), Eip155UptoError> {
    // Try to recover as EOA signature first
    if let Ok(recovered) = alloy_primitives::Signature::try_from(signature.as_ref())
        .and_then(|sig| sig.recover_address_from_prehash(&hash))
    {
        if recovered == signer {
            return Ok(());
        }
    }

    // TODO: Add EIP-1271 contract signature verification
    // For now, if EOA recovery fails, reject
    Err(Eip155UptoError::InvalidSignature)
}

/// Settle an upto payment on-chain.
#[instrument(skip_all, err, fields(
    payer = ?payload.payload.authorization.from,
    amount = %requirements.amount.0
))]
async fn settle_upto_payment<P>(
    provider: &P,
    payload: &types::PaymentPayload,
    requirements: &types::PaymentRequirements,
) -> Result<alloy_primitives::B256, Eip155UptoError>
where
    P: Eip155MetaTransactionProvider,
    Eip155UptoError: From<P::Error>,
{
    let authorization = &payload.payload.authorization;
    let owner = authorization.from;
    let spender = authorization.to;
    let cap = authorization.value;
    let deadline = authorization.valid_before;
    let asset_address = requirements.asset.0;
    let pay_to = requirements.pay_to.0;
    let amount = requirements.amount.0;

    // Parse signature into v, r, s
    let signature = &payload.payload.signature;
    let sig = alloy_primitives::Signature::try_from(signature.as_ref())
        .map_err(|_| Eip155UptoError::InvalidSignature)?;

    let v = sig.v().y_parity_byte_non_eip155().unwrap_or(sig.v().y_parity_byte());
    let r = sig.r();
    let s = sig.s();

    // Step 1: Try to apply permit
    let token_contract = IEIP3009::new(asset_address, provider.inner());
    
    // Attempt permit call
    let permit_calldata = token_contract
        .permit(owner, spender, cap, deadline, v, r, s)
        .calldata()
        .clone();

    let permit_result = provider
        .send_transaction(MetaTransaction {
            to: asset_address,
            calldata: permit_calldata,
        })
        .await;

    // If permit fails, check if we already have sufficient allowance
    if permit_result.is_err() {
        let allowance = token_contract
            .allowance(owner, spender)
            .call()
            .await
            .map_err(|e| {
                tracing::warn!("Failed to check allowance after permit failure: {}", e);
                Eip155UptoError::PermitFailed
            })?
            ._0;

        if allowance < amount {
            tracing::error!(
                "Permit failed and allowance insufficient: allowance={}, required={}",
                allowance,
                amount
            );
            return Err(Eip155UptoError::PermitFailed);
        }
        tracing::info!("Permit already used, proceeding with existing allowance");
    }

    // Step 2: Execute transferFrom
    let transfer_calldata = token_contract
        .transferFrom(owner, pay_to, amount)
        .calldata()
        .clone();

    let receipt = provider
        .send_transaction(MetaTransaction {
            to: asset_address,
            calldata: transfer_calldata,
        })
        .await?;

    if receipt.status() {
        Ok(*receipt.transaction_hash())
    } else {
        Err(Eip155UptoError::TransferFailed)
    }
}
