use alloy_provider::Provider;
use alloy_sol_types::Eip712Domain;
use x402_types::chain::{ChainId, ChainProviderOps};
use x402_types::proto::{PaymentVerificationError, v2};
use x402_types::scheme::X402SchemeFacilitatorError;

#[cfg(feature = "telemetry")]
use tracing::instrument;

use crate::chain::{Eip155ChainReference, Eip155MetaTransactionProvider};
use crate::v1_eip155_exact::{
    Eip155ExactError, ExactEvmPayment, IEIP3009, PaymentRequirementsExtra, assert_domain,
    assert_enough_balance, assert_enough_value, assert_time, settle_payment, verify_payment,
};
use crate::v2_eip155_exact::Eip3009Payload;
use crate::v2_eip155_exact::types::{Eip3009PaymentPayload, Eip3009PaymentRequirements};

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn verify_eip3009_payment<P: Eip155MetaTransactionProvider + ChainProviderOps>(
    provider: &P,
    payment_payload: &Eip3009PaymentPayload,
    payment_requirements: &Eip3009PaymentRequirements,
) -> Result<v2::VerifyResponse, X402SchemeFacilitatorError> {
    let accepted = &payment_payload.accepted;
    assert_requirements_match(accepted, payment_requirements)?;
    let (contract, payment, eip712_domain) = assert_valid_payment(
        provider.inner(),
        provider.chain(),
        accepted,
        &payment_payload.payload,
    )
    .await?;

    let payer = verify_payment(provider.inner(), &contract, &payment, &eip712_domain).await?;
    Ok(v2::VerifyResponse::valid(payer.to_string()))
}

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn settle_eip3009_payment<P>(
    provider: &P,
    payment_payload: &Eip3009PaymentPayload,
    payment_requirements: &Eip3009PaymentRequirements,
) -> Result<v2::SettleResponse, X402SchemeFacilitatorError>
where
    P: Eip155MetaTransactionProvider + ChainProviderOps,
    Eip155ExactError: From<P::Error>,
{
    let accepted = &payment_payload.accepted;
    assert_requirements_match(accepted, payment_requirements)?;
    let (contract, payment, eip712_domain) = assert_valid_payment(
        provider.inner(),
        provider.chain(),
        accepted,
        &payment_payload.payload,
    )
    .await?;

    let tx_hash = settle_payment(provider, &contract, &payment, &eip712_domain).await?;

    Ok(v2::SettleResponse::Success {
        payer: payment.from.to_string(),
        transaction: tx_hash.to_string(),
        network: accepted.network.to_string(),
    })
}

/// Runs all preconditions needed for a successful payment:
/// - Valid scheme, network, and receiver.
/// - Valid time window (validAfter/validBefore).
/// - Correct EIP-712 domain construction.
/// - Sufficient on-chain balance.
/// - Sufficient value in payload.
#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn assert_valid_payment<P: Provider>(
    provider: P,
    chain: &Eip155ChainReference,
    accepted: &Eip3009PaymentRequirements,
    payload: &Eip3009Payload,
) -> Result<(IEIP3009::IEIP3009Instance<P>, ExactEvmPayment, Eip712Domain), Eip155ExactError> {
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

    let amount_required = accepted.amount;
    assert_enough_value(&authorization.value, &amount_required)?;

    let extra = Some(PaymentRequirementsExtra {
        name: accepted.extra.name.clone(),
        version: accepted.extra.version.clone(),
    });
    let domain = assert_domain(chain, &contract, &asset_address.into(), &extra).await?;

    assert_enough_balance(&contract, &authorization.from, amount_required.into()).await?;

    let signature = payload.signature.clone();

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

pub fn assert_requirements_match<T: PartialEq>(
    accepted: &T,
    payment_requirements: &T,
) -> Result<(), PaymentVerificationError> {
    if accepted != payment_requirements {
        Err(PaymentVerificationError::AcceptedRequirementsMismatch)
    } else {
        Ok(())
    }
}
