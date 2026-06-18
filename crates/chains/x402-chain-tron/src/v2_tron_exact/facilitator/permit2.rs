//! Permit2 payment verification and settlement for TRON.

use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::{SolStruct, eip712_domain, sol};
use x402_types::proto::{PaymentVerificationError, v2};
use x402_types::scheme::X402SchemeFacilitatorError;
use x402_types::timestamp::UnixTimestamp;

use crate::chain::TronAddress;
use crate::chain::TronChainProvider;
use crate::chain::contracts;
use crate::chain::provider::{
    TronChainProviderError, TronChainProviderLike, read_allowance, read_balance_of,
};
use crate::v2_tron_exact::facilitator::eip3009::{assert_requirements_match, recover_address};
use crate::v2_tron_exact::types::{Permit2Payload, Permit2PaymentRequirements};

// Struct names are verbatim in the EIP-712 typehash — must match Permit2 exactly:
//   PermitWitnessTransferFrom(TokenPermissions permitted,address spender,
//     uint256 nonce,uint256 deadline,Witness witness)
//   TokenPermissions(address token,uint256 amount)
//   Witness(address to,uint256 validAfter)
sol! {
    struct TokenPermissions {
        address token;
        uint256 amount;
    }

    struct Witness {
        address to;
        uint256 validAfter;
    }

    struct PermitWitnessTransferFrom {
        TokenPermissions permitted;
        address spender;
        uint256 nonce;
        uint256 deadline;
        Witness witness;
    }
}

pub async fn verify_permit2_payment(
    provider: &TronChainProvider,
    payment_payload: &v2::PaymentPayload<Permit2PaymentRequirements, Permit2Payload>,
    payment_requirements: &Permit2PaymentRequirements,
) -> Result<v2::VerifyResponse, X402SchemeFacilitatorError> {
    let accepted = &payment_payload.accepted;
    let sun_permit2 = provider.sun_permit2;

    assert_requirements_match(accepted, payment_requirements)?;

    let auth = &payment_payload.payload.permit2_authorization;
    let now = UnixTimestamp::now();
    let required_amount: U256 = payment_payload.accepted.amount.into();

    if accepted.network != provider.chain_reference.chain_id() {
        return Err(PaymentVerificationError::ChainIdMismatch.into());
    }

    let auth_from = TronAddress::from(auth.from);
    if provider.is_signer(&auth_from) {
        return Err(PaymentVerificationError::InvalidSignature(
            "Payment from address must not be the facilitator".to_string(),
        )
        .into());
    }

    if auth.witness.to != Address::from(accepted.pay_to) {
        return Err(PaymentVerificationError::RecipientMismatch.into());
    }

    if auth.permitted.amount < accepted.amount {
        return Err(PaymentVerificationError::InvalidPaymentAmount.into());
    }

    if auth.permitted.token != Address::from(accepted.asset) {
        return Err(PaymentVerificationError::AssetMismatch.into());
    }

    if auth.deadline <= now + 6 {
        return Err(PaymentVerificationError::Expired.into());
    }
    if auth.witness.valid_after > now {
        return Err(PaymentVerificationError::Early.into());
    }

    // TIP-712 signature recovery against the Permit2 domain
    let permit2_evm = Address::from(sun_permit2);
    let chain_id = provider.chain_reference.inner().into();
    let domain = eip712_domain! {
        name: "Permit2",
        chain_id: chain_id,
        verifying_contract: permit2_evm,
    };
    let nonce_u256: U256 = auth.nonce.into();
    let deadline_u256 = U256::from(auth.deadline.as_secs());
    let valid_after_u256 = U256::from(auth.witness.valid_after.as_secs());
    let struct_data = PermitWitnessTransferFrom {
        permitted: TokenPermissions {
            token: auth.permitted.token,
            amount: auth.permitted.amount.into(),
        },
        spender: auth.spender,
        nonce: nonce_u256,
        deadline: deadline_u256,
        witness: Witness {
            to: auth.witness.to,
            validAfter: valid_after_u256,
        },
    };
    let hash = struct_data.eip712_signing_hash(&domain);
    let recovered = recover_address(hash.as_ref(), &payment_payload.payload.signature)
        .map_err(|e| PaymentVerificationError::InvalidSignature(e.to_string()))?;
    if recovered != auth.from {
        return Err(PaymentVerificationError::InvalidSignature(
            "Recovered signer does not match 'from'".to_string(),
        )
        .into());
    }

    let token = TronAddress::from(auth.permitted.token);
    let balance = read_balance_of(provider, token, auth.from)
        .await
        .map_err(|e| X402SchemeFacilitatorError::OnchainFailure(e.to_string()))?;
    if balance < required_amount {
        return Err(PaymentVerificationError::InsufficientFunds.into());
    }

    let allowance = read_allowance(provider, token, auth.from, permit2_evm)
        .await
        .map_err(|e| X402SchemeFacilitatorError::OnchainFailure(e.to_string()))?;
    if allowance < required_amount {
        return Err(PaymentVerificationError::InsufficientAllowance.into());
    }

    Ok(v2::VerifyResponse::valid(format!(
        "0x{}",
        alloy_primitives::hex::encode(auth.from)
    )))
}

pub async fn settle_permit2_payment(
    provider: &TronChainProvider,
    payment_payload: &v2::PaymentPayload<Permit2PaymentRequirements, Permit2Payload>,
    payment_requirements: &Permit2PaymentRequirements,
) -> Result<v2::SettleResponse, X402SchemeFacilitatorError> {
    verify_permit2_payment(provider, payment_payload, payment_requirements).await?;

    let accepted = &payment_payload.accepted;
    let auth = &payment_payload.payload.permit2_authorization;

    let txid = build_and_submit_permit2_settle_tx(
        provider,
        provider.x402_exact_permit2_proxy,
        auth.permitted.token,
        auth.permitted.amount.into(),
        auth.nonce.into(),
        auth.deadline,
        auth.from,
        auth.witness.to,
        auth.witness.valid_after,
        payment_payload.payload.signature.clone(),
    )
    .await
    .map_err(|e| X402SchemeFacilitatorError::OnchainFailure(e.to_string()))?;

    provider
        .wait_for_tx(&txid)
        .await
        .map_err(|e| X402SchemeFacilitatorError::OnchainFailure(e.to_string()))?;

    Ok(v2::SettleResponse::Success {
        payer: format!("0x{}", alloy_primitives::hex::encode(auth.from)),
        transaction: txid,
        network: accepted.network.to_string(),
    })
}

// ── Permit2 settlement tx ─────────────────────────────────────────────────────

pub async fn build_and_submit_permit2_settle_tx<P: TronChainProviderLike>(
    provider: &P,
    x402_exact_permit2_proxy: TronAddress,
    token: Address,
    amount: U256,
    nonce: U256,
    deadline: UnixTimestamp,
    owner: Address,
    witness_to: Address,
    witness_valid_after: UnixTimestamp,
    signature: Bytes,
) -> Result<String, TronChainProviderError> {
    use contracts::x402_exact_permit2_proxy as c;
    let settle_call = c::settleCall {
        permit: c::TronPermitTransferFrom {
            permitted: c::TronTokenPermissions { token, amount },
            nonce,
            deadline: U256::from(deadline.as_secs()),
        },
        owner,
        witness: c::TronWitness {
            to: witness_to,
            validAfter: U256::from(witness_valid_after.as_secs()),
        },
        signature,
    };
    provider
        .build_and_submit_tx(x402_exact_permit2_proxy, settle_call)
        .await
}
