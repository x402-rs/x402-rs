pub mod client;
pub mod types;

use alloy_provider::Provider;
use alloy_sol_types::Eip712Domain;
use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use tracing::instrument;

use crate::chain::eip155::{
    Eip155ChainProvider, Eip155ChainReference, Eip155MetaTransactionProvider,
};
use crate::chain::{ChainId, ChainProvider, ChainProviderOps};
use crate::proto;
use crate::proto::PaymentVerificationError;
use crate::proto::v2;
use crate::scheme::v1_eip155_exact::{
    Eip155ExactError, ExactEvmPayment, IEIP3009, assert_domain, assert_enough_balance,
    assert_enough_value, assert_time, settle_payment, verify_payment,
};
use crate::scheme::{
    X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError, X402SchemeId,
};

#[allow(unused)]
pub use types::*;

pub struct V2Eip155Exact;

impl X402SchemeId for V2Eip155Exact {
    fn namespace(&self) -> &str {
        "eip155"
    }

    fn scheme(&self) -> &str {
        types::ExactScheme.as_ref()
    }
}

impl X402SchemeFacilitatorBuilder for V2Eip155Exact {
    fn build(
        &self,
        provider: ChainProvider,
        _config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn Error>> {
        let provider = if let ChainProvider::Eip155(provider) = provider {
            provider
        } else {
            return Err("V2Eip155Exact::build: provider must be an Eip155ChainProvider".into());
        };
        Ok(Box::new(V2Eip155ExactFacilitator { provider }))
    }
}

pub struct V2Eip155ExactFacilitator {
    provider: Arc<Eip155ChainProvider>,
}

#[async_trait::async_trait]
impl X402SchemeFacilitator for V2Eip155ExactFacilitator {
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

        let tx_hash =
            settle_payment(self.provider.as_ref(), &contract, &payment, &eip712_domain).await?;

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
            x402_version: proto::X402Version::v2().into(),
            scheme: types::ExactScheme.to_string(),
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
#[instrument(skip_all, err)]
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
    let authorization = &payload.authorization;
    if authorization.to != accepted.pay_to {
        return Err(PaymentVerificationError::RecipientMismatch.into());
    }
    let valid_after = authorization.valid_after;
    let valid_before = authorization.valid_before;
    assert_time(valid_after, valid_before)?;
    let asset_address = accepted.asset;
    let contract = IEIP3009::new(asset_address.into(), provider);

    let domain = assert_domain(chain, &contract, &asset_address.into(), &accepted.extra).await?;

    let amount_required = accepted.amount;
    assert_enough_balance(&contract, &authorization.from, amount_required.into()).await?;
    assert_enough_value(&authorization.value, &amount_required.into())?;

    let payment = ExactEvmPayment {
        from: authorization.from,
        to: authorization.to,
        value: authorization.value,
        valid_after: authorization.valid_after,
        valid_before: authorization.valid_before,
        nonce: authorization.nonce,
        signature: payload.signature.clone(),
    };

    Ok((contract, payment, domain))
}
