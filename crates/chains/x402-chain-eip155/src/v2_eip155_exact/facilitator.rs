//! Facilitator-side payment verification and settlement for V2 EIP-155 exact scheme.
//!
//! This module implements the facilitator logic for V2 protocol payments on EVM chains.
//! It reuses most of the V1 verification and settlement logic but handles V2-specific
//! payload structures with embedded requirements and CAIP-2 chain IDs.

use alloy_provider::Provider;
use alloy_sol_types::Eip712Domain;
use std::collections::HashMap;
use x402_types::chain::{ChainId, ChainProviderOps};
use x402_types::proto;
use x402_types::proto::{PaymentVerificationError, v2};
use x402_types::scheme::{
    X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError,
};

use crate::V2Eip155Exact;
use crate::chain::{AssetTransferMethod, Eip155ChainReference, Eip155MetaTransactionProvider};
use crate::v1_eip155_exact::facilitator::{
    Eip155ExactError, ExactEvmPayment, IEIP3009, assert_domain, assert_enough_balance,
    assert_enough_value, assert_time, settle_payment, verify_payment,
};
use crate::v1_eip155_exact::{ExactScheme, PaymentRequirementsExtra};
use crate::v2_eip155_exact::types;

#[cfg(feature = "telemetry")]
use tracing::instrument;

impl<P> X402SchemeFacilitatorBuilder<P> for V2Eip155Exact
where
    P: Eip155MetaTransactionProvider + ChainProviderOps + Send + Sync + 'static,
    Eip155ExactError: From<P::Error>,
{
    fn build(
        &self,
        provider: P,
        _config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        Ok(Box::new(V2Eip155ExactFacilitator::new(provider)))
    }
}

/// Facilitator for V2 EIP-155 exact scheme payments.
///
/// This struct implements the [`X402SchemeFacilitator`] trait to provide payment
/// verification and settlement services for ERC-3009 based payments on EVM chains
/// using the V2 protocol.
///
/// # Type Parameters
///
/// - `P`: The provider type, which must implement [`Eip155MetaTransactionProvider`]
///   and [`ChainProviderOps`]
pub struct V2Eip155ExactFacilitator<P> {
    provider: P,
}

impl<P> V2Eip155ExactFacilitator<P> {
    /// Creates a new V2 EIP-155 exact scheme facilitator with the given provider.
    pub fn new(provider: P) -> Self {
        Self { provider }
    }
}

#[async_trait::async_trait]
impl<P> X402SchemeFacilitator for V2Eip155ExactFacilitator<P>
where
    P: Eip155MetaTransactionProvider + ChainProviderOps + Send + Sync,
    P::Inner: Provider,
    Eip155ExactError: From<P::Error>,
{
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError> {
        let request = types::VerifyRequest::from_proto(request.clone())?;
        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;
        let (contract, payment, eip712_domain) = assert_valid_payment(
            self.provider.inner(),
            self.provider.chain(),
            payload,
            requirements,
        )
        .await?;

        let payer =
            verify_payment(self.provider.inner(), &contract, &payment, &eip712_domain).await?;
        Ok(v2::VerifyResponse::valid(payer.to_string()).into())
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, X402SchemeFacilitatorError> {
        let request = types::SettleRequest::from_proto(request.clone())?;
        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;
        let (contract, payment, eip712_domain) = assert_valid_payment(
            self.provider.inner(),
            self.provider.chain(),
            payload,
            requirements,
        )
        .await?;

        let tx_hash = settle_payment(&self.provider, &contract, &payment, &eip712_domain).await?;

        Ok(v2::SettleResponse::Success {
            payer: payment.from.to_string(),
            transaction: tx_hash.to_string(),
            network: payload.accepted.network.to_string(),
        }
        .into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError> {
        let chain_id = self.provider.chain_id();
        let kinds = vec![proto::SupportedPaymentKind {
            x402_version: v2::X402Version2.into(),
            scheme: ExactScheme.to_string(),
            network: chain_id.clone().into(),
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

/// Runs all preconditions needed for a successful payment:
/// - Valid scheme, network, and receiver.
/// - Valid time window (validAfter/validBefore).
/// - Correct EIP-712 domain construction.
/// - Sufficient on-chain balance.
/// - Sufficient value in payload.
#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
async fn assert_valid_payment<P: Provider>(
    provider: P,
    chain: &Eip155ChainReference,
    payload: &types::PaymentPayload,
    requirements: &types::PaymentRequirements,
) -> Result<(IEIP3009::IEIP3009Instance<P>, ExactEvmPayment, Eip712Domain), Eip155ExactError> {
    let accepted = &payload.accepted;
    if accepted != requirements {
        return Err(PaymentVerificationError::AcceptedRequirementsMismatch.into());
    }
    let payload = &payload.payload;

    let chain_id: ChainId = chain.into();
    let payload_chain_id = &accepted.network;
    if payload_chain_id != &chain_id {
        return Err(PaymentVerificationError::ChainIdMismatch.into());
    }
    let authorization = match &payload {
        types::ExactEvmPayload::Eip3009(payload) => payload.authorization,
        types::ExactEvmPayload::Permit2(payload) => {
            todo!("Permit2 is not yet supported")
        }
    };
    if authorization.to != accepted.pay_to {
        return Err(PaymentVerificationError::RecipientMismatch.into());
    }
    let valid_after = authorization.valid_after;
    let valid_before = authorization.valid_before;
    assert_time(valid_after, valid_before)?;
    let asset_address = accepted.asset;
    let contract = IEIP3009::new(asset_address.into(), provider);

    let extra = match &accepted.extra {
        AssetTransferMethod::Eip3009 { name, version } => Some(PaymentRequirementsExtra {
            name: name.clone(),
            version: version.clone(),
        }),
        AssetTransferMethod::Permit2 => {
            todo!("Permit2 is not yet supported")
        }
    };

    let domain = assert_domain(chain, &contract, &asset_address.into(), &extra).await?;

    let amount_required = accepted.amount;
    assert_enough_balance(&contract, &authorization.from, amount_required.into()).await?;
    assert_enough_value(&authorization.value.into(), &amount_required.into())?;

    let signature = match &payload {
        types::ExactEvmPayload::Eip3009(payload) => payload.signature.clone(),
        types::ExactEvmPayload::Permit2(payload) => {
            todo!("Permit2 is not yet supported")
        }
    };

    let payment = ExactEvmPayment {
        from: authorization.from,
        to: authorization.to,
        value: authorization.value.into(),
        valid_after: authorization.valid_after,
        valid_before: authorization.valid_before,
        nonce: authorization.nonce,
        signature,
    };

    Ok((contract, payment, domain))
}
