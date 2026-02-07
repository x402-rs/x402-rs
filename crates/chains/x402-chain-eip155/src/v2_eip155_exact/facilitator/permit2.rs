use alloy_primitives::{Address, Bytes, U256, address, hex};
use alloy_provider::Provider;
use alloy_sol_types::{Eip712Domain, SolStruct, eip712_domain, sol};
use std::str::FromStr;
use x402_types::chain::ChainProviderOps;
use x402_types::proto::{PaymentVerificationError, v2};
use x402_types::scheme::X402SchemeFacilitatorError;

#[cfg(feature = "telemetry")]
use tracing::Instrument;
#[cfg(feature = "telemetry")]
use tracing::instrument;

use crate::chain::Eip155MetaTransactionProvider;
use crate::v1_eip155_exact::{
    Eip155ExactError, StructuredSignature, assert_enough_value, assert_time,
};
use crate::v2_eip155_exact::eip3009::assert_requirements_match;
use crate::v2_eip155_exact::types::{
    Permit2Payload, Permit2PaymentPayload, Permit2PaymentRequirements,
};

pub const EXACT_PERMIT2_PROXY_ADDRESS: Address =
    address!("0x4020615294c913F045dc10f0a5cdEbd86c280001");

pub const PERMIT2_ADDRESS: Address = address!("0x000000000022D473030F116dDEE9F6B43aC78BA3");

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    #[sol(rpc)]
    IERC20Permit,
    "abi/IERC20Permit.json"
);

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    #[sol(rpc)]
    IERC20,
    "abi/IERC20.json"
);

sol!(
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    struct PermitWitnessTransferFrom {
        TokenPermissions permitted;
        address spender;
        uint256 nonce;
        uint256 deadline;
        Witness witness;
    }

    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    struct TokenPermissions {
        address token;
        uint256 amount;
    }

    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    struct Witness {
        address to;
        uint256 validAfter;
        bytes extra;
    }
);

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn verify_permit2_payment<P: Eip155MetaTransactionProvider + ChainProviderOps>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
) -> Result<v2::VerifyResponse, Eip155ExactError> {
    assert_offchain(payment_payload, payment_requirements)?;

    let authorization = &payment_payload.payload.permit_2_authorization;
    let payer: Address = authorization.from.into();
    let required_amount: U256 = payment_payload.accepted.amount.into();
    let asset_address: Address = payment_payload.accepted.asset.into();

    let token_contract = IERC20::new(asset_address, provider.inner());

    // Allowance from payer to Permit2 contract is enough
    assert_onchain_allowance(&token_contract, payer, required_amount).await?;
    // User balance is enough
    assert_onchain_balance(&token_contract, payer, required_amount).await?;

    let chain_reference = provider.chain().inner();
    let domain = eip712_domain! {
        name: "Permit2",
        chain_id: chain_reference,
        verifying_contract: PERMIT2_ADDRESS,
    };
    let transfer = PermitWitnessTransferFrom {
        permitted: TokenPermissions {
            token: authorization.permitted.token.into(),
            amount: authorization.permitted.amount.into(),
        },
        spender: EXACT_PERMIT2_PROXY_ADDRESS,
        nonce: authorization.nonce.into(),
        deadline: U256::from(authorization.deadline.as_secs()),
        witness: Witness {
            to: authorization.witness.to.into(),
            validAfter: U256::from(authorization.witness.valid_after.as_secs()),
            extra: authorization.witness.extra.clone(),
        },
    };
    let eip712_hash = transfer.eip712_signing_hash(&domain);
    let structured_signature: StructuredSignature = StructuredSignature::try_from_bytes(
        payment_payload.payload.signature.clone(),
        payer,
        &eip712_hash,
    )?;
    println!("s.9 {:?}", structured_signature);

    match structured_signature {
        StructuredSignature::EIP6492 { .. } => {
            todo!("EIP6492 signature verification")
        }
        StructuredSignature::EOA(_) => {
            todo!("EOA signature verification")
        }
        StructuredSignature::EIP1271(_) => {
            todo!("EIP1271 signature verification")
        }
    }

    // TODO signature

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

pub fn assert_offchain(
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
