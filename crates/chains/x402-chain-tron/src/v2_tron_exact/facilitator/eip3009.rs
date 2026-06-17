//! EIP-3009 payment verification and settlement for TRON.
//!
//! TRON uses TIP-712 (identical to EIP-712) for typed data signing.
//! Addresses in the authorization payload are EVM hex (0x...); addresses in
//! PaymentRequirements are Base58Check (TronAddress).

use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::{Eip712Domain, SolStruct, eip712_domain, sol};
use x402_types::chain::ChainId;
use x402_types::proto::{PaymentVerificationError, v2};
use x402_types::scheme::X402SchemeFacilitatorError;
use x402_types::timestamp::UnixTimestamp;

use crate::chain::provider::TronChainProviderLike;
use crate::chain::{TronAddress, TronChainProvider, TronChainReference};
use crate::v2_tron_exact::types::{Eip3009Payload, Eip3009PaymentRequirements};
use crate::v2_tron_exact::{Eip3009Authorization, Eip3009PaymentPayload};

sol! {
    struct TransferWithAuthorization {
        address from;
        address to;
        uint256 value;
        uint256 validAfter;
        uint256 validBefore;
        bytes32 nonce;
    }
}

pub async fn verify_eip3009_payment(
    provider: &TronChainProvider,
    payment_payload: &Eip3009PaymentPayload,
    payment_requirements: &Eip3009PaymentRequirements,
) -> Result<v2::VerifyResponse, X402SchemeFacilitatorError> {
    let accepted = &payment_payload.accepted;
    assert_requirements_match(accepted, payment_requirements)?;
    assert_valid_payment(
        provider,
        &provider.chain_reference,
        accepted,
        payment_payload,
    ).await?;

    let auth = &payment_payload.payload.authorization;
    let required_amount = accepted.amount;

    let token = &accepted.asset;
    let balance = provider
        .read_balance_of(token, auth.from)
        .await
        .map_err(|e| X402SchemeFacilitatorError::OnchainFailure(e.to_string()))?;
    if balance < required_amount.0 {
        return Err(PaymentVerificationError::InsufficientFunds.into());
    }

    if provider
        .read_authorization_state(token, auth.from, auth.nonce)
        .await
        .map_err(|e| X402SchemeFacilitatorError::OnchainFailure(e.to_string()))?
    {
        return Err(PaymentVerificationError::InvalidSignature(
            "Authorization nonce already used".to_string(),
        )
        .into());
    }

    let sim_ok = provider
        .simulate_transfer_with_authorization(
            token,
            auth.from,
            auth.to,
            auth.value.into(),
            auth.valid_after,
            auth.valid_before,
            auth.nonce,
            payment_payload.payload.signature.clone(),
        )
        .await
        .map_err(|e| X402SchemeFacilitatorError::OnchainFailure(e.to_string()))?;
    if !sim_ok {
        return Err(PaymentVerificationError::TransactionSimulation(
            "transferWithAuthorization simulation failed".to_string(),
        )
        .into());
    }

    Ok(v2::VerifyResponse::valid(format!(
        "0x{}",
        alloy_primitives::hex::encode(auth.from)
    )))
}

pub async fn settle_eip3009_payment(
    provider: &TronChainProvider,
    payment_payload: &v2::PaymentPayload<Eip3009PaymentRequirements, Eip3009Payload>,
    payment_requirements: &Eip3009PaymentRequirements,
) -> Result<v2::SettleResponse, X402SchemeFacilitatorError> {
    verify_eip3009_payment(provider, payment_payload, payment_requirements).await?;

    let accepted = &payment_payload.accepted;
    let auth = &payment_payload.payload.authorization;

    let txid = provider
        .build_and_submit_eip3009_tx(
            &accepted.asset,
            auth.from,
            auth.to,
            auth.value.into(),
            auth.valid_after,
            auth.valid_before,
            auth.nonce,
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

// ── Helpers ───────────────────────────────────────────────────────────────────

fn recover_eip3009_signer(
    domain: &Eip712Domain,
    auth: &Eip3009Authorization,
    signature: &Bytes,
) -> Result<Address, String> {
    let hash = TransferWithAuthorization {
        from: auth.from,
        to: auth.to,
        value: auth.value.into(),
        validAfter: U256::from(auth.valid_after.as_secs()),
        validBefore: U256::from(auth.valid_before.as_secs()),
        nonce: auth.nonce,
    }
    .eip712_signing_hash(domain);
    recover_address(hash.as_ref(), signature)
}

pub(crate) fn recover_address(hash: &[u8; 32], signature: &Bytes) -> Result<Address, String> {
    use k256::ecdsa::{RecoveryId, Signature as K256Sig, VerifyingKey};

    if signature.len() != 65 {
        return Err(format!(
            "signature must be 65 bytes, got {}",
            signature.len()
        ));
    }
    let rec_id = {
        let v = signature[64];
        RecoveryId::try_from(if v >= 27 { v - 27 } else { v })
            .map_err(|e| format!("invalid recovery id: {e}"))?
    };
    let sig = K256Sig::from_slice(&signature[..64])
        .map_err(|e| format!("invalid signature bytes: {e}"))?;
    let vk = VerifyingKey::recover_from_prehash(hash, &sig, rec_id)
        .map_err(|e| format!("signature recovery failed: {e}"))?;
    let point = vk.to_encoded_point(false);
    let keccak = alloy_primitives::keccak256(&point.as_bytes()[1..]);
    Ok(Address::from_slice(&keccak[12..]))
}

pub fn assert_requirements_match<T: PartialEq>(
    accepted: &T,
    requirements: &T,
) -> Result<(), PaymentVerificationError> {
    if accepted != requirements {
        Err(PaymentVerificationError::AcceptedRequirementsMismatch)
    } else {
        Ok(())
    }
}

pub async fn assert_valid_payment<P>(
    provider: &P,
    chain: &TronChainReference,
    accepted: &Eip3009PaymentRequirements,
    payload: &Eip3009PaymentPayload,
) -> Result<(), X402SchemeFacilitatorError>
where
    P: TronChainProviderLike,
{
    let chain_id: ChainId = ChainId::from(chain);
    let payload_chain_id = &accepted.network;
    if payload_chain_id != &chain_id {
        return Err(PaymentVerificationError::ChainIdMismatch.into());
    }

    let auth = &payload.payload.authorization;
    let now = UnixTimestamp::now();

    // From the spec: Facilitator safety: the facilitator's address MUST NOT appear as from (eip3009) or permit2Authorization.from (permit2) in the signed payload.
    let authorization_from = TronAddress::from(auth.from);
    if provider.is_signer(&authorization_from) {
        return Err(PaymentVerificationError::InvalidSignature(
            "Payment from address must not be the facilitator".to_string(),
        )
        .into());
    }

    let authorization_to = TronAddress::from(auth.to);
    if authorization_to != accepted.pay_to {
        return Err(PaymentVerificationError::RecipientMismatch.into());
    }

    let required_amount = accepted.amount;
    if auth.value < required_amount {
        return Err(PaymentVerificationError::InvalidPaymentAmount.into());
    }

    if now <= auth.valid_after {
        return Err(PaymentVerificationError::Early.into());
    }
    if now >= auth.valid_before {
        return Err(PaymentVerificationError::Expired.into());
    }

    let domain = eip712_domain! {
        name: accepted.extra.name.clone(),
        version: accepted.extra.version.clone(),
        chain_id: provider.chain().into(),
        verifying_contract: Address::from(accepted.asset),
    };
    let recovered = recover_eip3009_signer(&domain, auth, &payload.payload.signature)
        .map_err(|e| PaymentVerificationError::InvalidSignature(e.to_string()))?;

    if recovered != auth.from {
        return Err(PaymentVerificationError::InvalidSignature(
            "Recovered signer does not match 'from'".to_string(),
        )
        .into());
    }

    Ok(())
}
