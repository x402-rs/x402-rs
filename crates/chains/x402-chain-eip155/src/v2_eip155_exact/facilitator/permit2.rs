use alloy_primitives::{Address, U256, address};
use alloy_provider::Provider;
use alloy_sol_types::{SolStruct, eip712_domain, sol};
use x402_types::chain::ChainProviderOps;
use x402_types::proto::{PaymentVerificationError, v2};
use x402_types::scheme::X402SchemeFacilitatorError;

#[cfg(feature = "telemetry")]
use tracing::Instrument;
#[cfg(feature = "telemetry")]
use tracing::instrument;

use crate::chain::{Eip155ChainReference, Eip155MetaTransactionProvider};
use crate::v1_eip155_exact::{
    Eip155ExactError, StructuredSignature, assert_enough_value, assert_time,
};
use crate::v2_eip155_exact::eip3009::assert_requirements_match;
use crate::v2_eip155_exact::types::{Permit2PaymentPayload, Permit2PaymentRequirements};

// Note: Expect deployed on every chain
pub const EXACT_PERMIT2_PROXY_ADDRESS: Address =
    address!("0x4020615294c913F045dc10f0a5cdEbd86c280001");

// Note: Expect deployed on every chain
pub const PERMIT2_ADDRESS: Address = address!("0x000000000022D473030F116dDEE9F6B43aC78BA3");

// FIXME Remove this
// sol!(
//     #[allow(missing_docs)]
//     #[allow(clippy::too_many_arguments)]
//     #[derive(Debug)]
//     #[sol(rpc)]
//     IERC20Permit,
//     "abi/IERC20Permit.json"
// );

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    #[sol(rpc)]
    IERC20,
    "abi/IERC20.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    #[sol(rpc)]
    X402ExactPermit2Proxy,
    "abi/X402ExactPermit2Proxy.json"
);

sol!(
    /// Signature struct to do settle through [`X402ExactPermit2Proxy`]
    /// Depends on availability of [`X402ExactPermit2Proxy`]
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    struct PermitWitnessTransferFrom {
        ISignatureTransfer.TokenPermissions permitted;
        address spender;
        uint256 nonce;
        uint256 deadline;
        x402BasePermit2Proxy.Witness witness;
    }
);

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn verify_permit2_payment<P: Eip155MetaTransactionProvider + ChainProviderOps>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
) -> Result<v2::VerifyResponse, Eip155ExactError> {
    // 1. Verify offchain constraints
    assert_offchain_valid(payment_payload, payment_requirements)?;

    // 2. Verify onchain constraints
    let authorization = &payment_payload.payload.permit_2_authorization;
    let payer: Address = authorization.from.into();
    assert_onchain_valid(provider.inner(), provider.chain(), payment_payload).await?;

    Ok(v2::VerifyResponse::valid(payer.to_string()))
}

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn settle_permit2_payment<P: Eip155MetaTransactionProvider + ChainProviderOps>(
    _provider: &P,
    _payment_payload: &Permit2PaymentPayload,
    _payment_requirements: &Permit2PaymentRequirements,
) -> Result<v2::SettleResponse, X402SchemeFacilitatorError> {
    todo!("Permit2 - settle_permit2_payment")
}

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub fn assert_offchain_valid(
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
) -> Result<(), PaymentVerificationError> {
    let payload = &payment_payload.payload;
    let accepted = &payment_payload.accepted;
    assert_requirements_match(accepted, payment_requirements)?;

    // Spender must be the x402ExactPermit2Proxy contract address
    let authorization = &payload.permit_2_authorization;
    if authorization.spender.0 != EXACT_PERMIT2_PROXY_ADDRESS {
        return Err(PaymentVerificationError::RecipientMismatch);
    }

    // Correct recipient
    let witness = &authorization.witness;
    if witness.to != accepted.pay_to {
        return Err(PaymentVerificationError::RecipientMismatch);
    }

    // Time validity
    let valid_after = witness.valid_after;
    let valid_before = authorization.deadline;
    assert_time(valid_after, valid_before)?;

    // Sufficient amount
    let amount_required = &accepted.amount;
    assert_enough_value(&authorization.permitted.amount, amount_required)?;

    // Same token
    if authorization.permitted.token != accepted.asset {
        return Err(PaymentVerificationError::AssetMismatch);
    }
    Ok(())
}

pub async fn assert_onchain_allowance<P: Provider>(
    token_contract: &IERC20::IERC20Instance<P>,
    payer: Address,
    required_amount: U256,
) -> Result<(), Eip155ExactError> {
    let allowance_call = token_contract.allowance(payer, PERMIT2_ADDRESS);
    let allowance_fut = allowance_call.call().into_future();
    #[cfg(feature = "telemetry")]
    let allowance = allowance_fut
        .instrument(tracing::info_span!(
            "fetch_permit2_allowance",
            token_contract = %token_contract.address(),
            sender = %payer,
            otel.kind = "client"
        ))
        .await?;
    #[cfg(not(feature = "telemetry"))]
    let allowance = allowance_fut.await?;
    if allowance < required_amount {
        return Err(PaymentVerificationError::InsufficientAllowance.into());
    }
    Ok(())
}

pub async fn assert_onchain_balance<P: Provider>(
    token_contract: &IERC20::IERC20Instance<P>,
    payer: Address,
    required_amount: U256,
) -> Result<(), Eip155ExactError> {
    let balance_call = token_contract.balanceOf(payer);
    let balance_fut = balance_call.call().into_future();
    #[cfg(feature = "telemetry")]
    let balance = balance_fut
        .instrument(tracing::info_span!(
            "fetch_balance",
            token_contract = %token_contract.address(),
            sender = %payer,
            otel.kind = "client"
        ))
        .await?;
    #[cfg(not(feature = "telemetry"))]
    let balance = balance_fut.await?;
    if balance < required_amount {
        return Err(PaymentVerificationError::InsufficientFunds.into());
    }
    Ok(())
}

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn assert_onchain_valid<P: Provider>(
    provider: &P,
    chain_reference: &Eip155ChainReference,
    payment_payload: &Permit2PaymentPayload,
) -> Result<(), Eip155ExactError> {
    let authorization = &payment_payload.payload.permit_2_authorization;
    let payer = authorization.from.0;
    let required_amount = payment_payload.accepted.amount.0;
    let asset_address = payment_payload.accepted.asset.0;

    let token_contract = IERC20::new(asset_address, provider);

    // Allowance from payer to Permit2 contract is enough
    let onchain_allowance_fut = assert_onchain_allowance(&token_contract, payer, required_amount);
    // User balance is enough
    let onchain_balance_fut = assert_onchain_balance(&token_contract, payer, required_amount);
    tokio::try_join!(onchain_allowance_fut, onchain_balance_fut)?;

    // ... and below is a check if we can do the settle

    let domain = eip712_domain! {
        name: "Permit2",
        chain_id: chain_reference.inner(),
        verifying_contract: PERMIT2_ADDRESS,
    };
    let permit_witness_transfer_from = PermitWitnessTransferFrom {
        permitted: ISignatureTransfer::TokenPermissions {
            token: authorization.permitted.token.into(),
            amount: authorization.permitted.amount.into(),
        },
        spender: EXACT_PERMIT2_PROXY_ADDRESS,
        nonce: authorization.nonce.into(),
        deadline: U256::from(authorization.deadline.as_secs()),
        witness: x402BasePermit2Proxy::Witness {
            to: authorization.witness.to.into(),
            validAfter: U256::from(authorization.witness.valid_after.as_secs()),
            extra: authorization.witness.extra.clone(),
        },
    };
    let eip712_hash = permit_witness_transfer_from.eip712_signing_hash(&domain);
    let structured_signature = StructuredSignature::try_from_bytes(
        payment_payload.payload.signature.clone(),
        payer,
        &eip712_hash,
    )?;

    let exact_permit2_proxy = X402ExactPermit2Proxy::new(EXACT_PERMIT2_PROXY_ADDRESS, provider);
    match structured_signature {
        StructuredSignature::EIP6492 { .. } => {
            // FIXME TODO EIP6492 signature
            Err(PaymentVerificationError::InvalidFormat(
                "EIP6492 signature is not supported".to_string(),
            )
            .into())
        }
        StructuredSignature::EOA(signature) => {
            let permit_transfer_from = ISignatureTransfer::PermitTransferFrom {
                permitted: permit_witness_transfer_from.permitted,
                nonce: permit_witness_transfer_from.nonce,
                deadline: permit_witness_transfer_from.deadline,
            };
            let witness = permit_witness_transfer_from.witness;
            let settle_call = exact_permit2_proxy.settle(
                permit_transfer_from,
                payer,
                witness,
                signature.as_bytes().into(),
            );
            let settle_call_fut = settle_call.call().into_future();
            #[cfg(feature = "telemetry")]
            settle_call_fut
                .instrument(tracing::info_span!("call_settle_exact_permit2",
                    from = %payer,
                    to = %authorization.witness.to,
                    value = %authorization.permitted.amount,
                    valid_after = %authorization.witness.valid_after,
                    valid_before = %authorization.deadline,
                    nonce = %authorization.nonce,
                    signature = %signature,
                    token_contract = %authorization.permitted.token,
                    otel.kind = "client",
                ))
                .await?;
            #[cfg(not(feature = "telemetry"))]
            settle_call_fut.await?;
            Ok(())
        }
        StructuredSignature::EIP1271(_) => {
            // FIXME TODO EIP1271 signature
            Err(PaymentVerificationError::InvalidFormat(
                "EIP1271 signature is not supported".to_string(),
            )
            .into())
        }
    }
}
