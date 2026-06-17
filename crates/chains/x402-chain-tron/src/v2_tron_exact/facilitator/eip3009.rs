//! EIP-3009 payment verification and settlement for TRON.
//!
//! TRON uses TIP-712 (identical to EIP-712) for typed data signing.
//! Addresses in the authorization payload are EVM hex (0x...); addresses in
//! PaymentRequirements are Base58Check (TronAddress).

use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::{Eip712Domain, SolStruct, sol};
use x402_types::proto::{PaymentVerificationError, v2};
use x402_types::scheme::X402SchemeFacilitatorError;
use x402_types::timestamp::UnixTimestamp;

use crate::v2_tron_exact::Eip3009Authorization;
use crate::chain::TronChainProvider;
use crate::v2_tron_exact::types::{Eip3009Payload, Eip3009PaymentRequirements};

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

pub async fn verify_eip3009(
    provider: &TronChainProvider,
    payment_payload: &v2::PaymentPayload<Eip3009PaymentRequirements, Eip3009Payload>,
    payment_requirements: &Eip3009PaymentRequirements,
) -> Result<v2::VerifyResponse, X402SchemeFacilitatorError> {
    let accepted = &payment_payload.accepted;
    assert_requirements_match(accepted, payment_requirements)?;

    let auth = &payment_payload.payload.authorization;
    let now = UnixTimestamp::now();

    if accepted.network != provider.chain_reference.chain_id() {
        return Err(PaymentVerificationError::ChainIdMismatch.into());
    }

    if provider.is_signer(&auth.from) {
        return Err(PaymentVerificationError::InvalidSignature(
            "Payment from address must not be the facilitator".to_string(),
        )
        .into());
    }

    if auth.to != Address::from(accepted.pay_to) {
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

    // TIP-712 signature recovery
    let domain = build_eip712_domain(
        provider.eip712_chain_id(),
        &accepted.extra.name,
        &accepted.extra.version,
        Address::from(accepted.asset),
    );
    let recovered = recover_eip3009_signer(&domain, auth, &payment_payload.payload.signature)
        .map_err(|e| PaymentVerificationError::InvalidSignature(e.to_string()))?;
    if recovered != auth.from {
        return Err(PaymentVerificationError::InvalidSignature(
            "Recovered signer does not match 'from'".to_string(),
        )
        .into());
    }

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

pub async fn settle_eip3009(
    provider: &TronChainProvider,
    payment_payload: &v2::PaymentPayload<Eip3009PaymentRequirements, Eip3009Payload>,
    payment_requirements: &Eip3009PaymentRequirements,
) -> Result<v2::SettleResponse, X402SchemeFacilitatorError> {
    verify_eip3009(provider, payment_payload, payment_requirements).await?;

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

fn build_eip712_domain(chain_id: u64, name: &str, version: &str, token: Address) -> Eip712Domain {
    Eip712Domain {
        name: Some(std::borrow::Cow::Owned(name.to_owned())),
        version: Some(std::borrow::Cow::Owned(version.to_owned())),
        chain_id: Some(U256::from(chain_id)),
        verifying_contract: Some(token),
        salt: None,
    }
}

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
