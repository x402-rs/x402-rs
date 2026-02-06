use alloy_primitives::{Address, address};
use x402_types::chain::ChainProviderOps;
use x402_types::proto::{PaymentVerificationError, v2};
use x402_types::scheme::X402SchemeFacilitatorError;

#[cfg(feature = "telemetry")]
use tracing::instrument;

use crate::chain::Eip155MetaTransactionProvider;
use crate::v1_eip155_exact::{assert_enough_value, assert_time};
use crate::v2_eip155_exact::eip3009::assert_requirements_match;
use crate::v2_eip155_exact::{Permit2PaymentPayload, Permit2PaymentRequirements};

pub const EXACT_PERMIT2_PROXY_ADDRESS: Address =
    address!("0x4020615294c913F045dc10f0a5cdEbd86c280001");

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn verify_permit2_payment<P: Eip155MetaTransactionProvider + ChainProviderOps>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
) -> Result<v2::VerifyResponse, PaymentVerificationError> {
    // Accepted must match the requirements
    let accepted = &payment_payload.accepted;
    assert_requirements_match(accepted, payment_requirements)?;

    // Spender must be the x402ExactPermit2Proxy contract address
    let authorization = &payment_payload.payload.permit_2_authorization;
    if authorization.spender.0 != EXACT_PERMIT2_PROXY_ADDRESS {
        return Err(PaymentVerificationError::RecipientMismatch);
    }

    // Correct recipient
    let witness = &authorization.witness;
    if witness.to != payment_requirements.pay_to {
        return Err(PaymentVerificationError::RecipientMismatch);
    }

    // Time validity
    let valid_after = witness.valid_after;
    let valid_before = authorization.deadline;
    assert_time(valid_after, valid_before)?;

    // Sufficient amount
    let amount_required = &payment_requirements.amount;
    assert_enough_value(&authorization.permitted.amount, amount_required)?;

    // Same token
    if authorization.permitted.token != payment_requirements.asset {
        return Err(PaymentVerificationError::AssetMismatch);
    }

    println!("---- Permit2 - verify_permit2_payment");

    todo!("Permit2 - verify_permit2_payment")
}

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn settle_permit2_payment<P: Eip155MetaTransactionProvider + ChainProviderOps>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
) -> Result<v2::SettleResponse, X402SchemeFacilitatorError> {
    todo!("Permit2 - settle_permit2_payment")
}
