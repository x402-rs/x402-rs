//! Settle dispatcher for the batch-settlement scheme.
//!
//! Drives the four settlement actions the facilitator can perform on behalf
//! of clients and servers:
//!
//! - `deposit`  — submit `deposit(config, amount, collector, collectorData)`
//! - `claim`    — submit `claimWithSignature(claims, authorizerSignature)`
//! - `settle`   — submit `settle(receiver, token)`
//! - `refund`   — submit `refundWithSignature(config, amount, nonce, sig)`
//!
//! On success the response carries the transaction hash and an `extra` block
//! with the post-transaction channel snapshot when applicable.

use alloy_primitives::{Address, B256, Bytes, U128, U256};
use alloy_provider::Provider;
use alloy_sol_types::SolCall;

use super::abi::X402BatchSettlement::{
    claimCall, claimWithSignatureCall, depositCall, multicallCall, refundWithSignatureCall,
    settleCall,
};
use super::authorizer_signer::{
    ReceiverAuthorizerSigner, compute_claim_batch_digest, compute_refund_digest,
    to_abi_voucher_claims,
};
use super::response::BatchSettlementSettleExtra;
use super::response::BatchSettlementSettleResponse;
use super::utils::{compute_channel_id, read_channel_state, to_abi_channel_config};
use super::verify::verify;
use crate::chain::{Eip155MetaTransactionProvider, MetaTransaction, MetaTransactionSendError};
use crate::v2_eip155_batch_settlement::constants::{
    BATCH_SETTLEMENT_ADDRESS, ERC3009_DEPOSIT_COLLECTOR_ADDRESS, PERMIT2_DEPOSIT_COLLECTOR_ADDRESS,
};
use crate::v2_eip155_batch_settlement::encoding::{
    build_erc3009_collector_data, build_permit2_collector_data,
};
use crate::v2_eip155_batch_settlement::errors as err;
use crate::v2_eip155_batch_settlement::types::{
    BatchSettlementPayload, BatchSettlementRefundPayload, ChannelStateExtra, ClaimPayload,
    DepositAuthorization, DepositPayload, EnrichedRefundPayload, PaymentPayload,
    PaymentRequirements, SettlePayload,
};

/// Top-level settle dispatcher.
pub async fn settle<P>(
    provider: &P,
    chain_id: u64,
    receiver_authorizer: Option<&ReceiverAuthorizerSigner>,
    payment_payload: &PaymentPayload,
    requirements: &PaymentRequirements,
) -> BatchSettlementSettleResponse
where
    P: Eip155MetaTransactionProvider + Send + Sync,
    P::Inner: Provider,
    P::Error: Into<MetaTransactionSendError>,
{
    let network = requirements.network.to_string();

    if payment_payload.accepted.network != requirements.network {
        return BatchSettlementSettleResponse::failure(network, err::ERR_NETWORK_MISMATCH);
    }

    match &payment_payload.payload {
        BatchSettlementPayload::Deposit(deposit) => {
            settle_deposit(provider, chain_id, payment_payload, deposit, requirements).await
        }
        BatchSettlementPayload::Claim(claim) => {
            settle_claim(provider, chain_id, receiver_authorizer, claim, requirements).await
        }
        BatchSettlementPayload::Settle(payload) => {
            settle_transfer(provider, payload, requirements).await
        }
        BatchSettlementPayload::Refund(BatchSettlementRefundPayload::Enriched(enriched)) => {
            settle_refund(
                provider,
                chain_id,
                receiver_authorizer,
                enriched,
                requirements,
            )
            .await
        }
        BatchSettlementPayload::Refund(BatchSettlementRefundPayload::Client(_))
        | BatchSettlementPayload::Voucher(_) => {
            // Bare client refund / voucher payloads must be enriched and
            // promoted to a settle action by the server first.
            BatchSettlementSettleResponse::failure(network, err::ERR_INVALID_PAYLOAD_TYPE)
        }
    }
}

async fn settle_deposit<P>(
    provider: &P,
    chain_id: u64,
    payment_payload: &PaymentPayload,
    payload: &DepositPayload,
    requirements: &PaymentRequirements,
) -> BatchSettlementSettleResponse
where
    P: Eip155MetaTransactionProvider + Send + Sync,
    P::Inner: Provider,
    P::Error: Into<MetaTransactionSendError>,
{
    let network = requirements.network.to_string();
    // 1. Verify the payload first — the same checks that ran on `/verify`
    //    must hold here, and we get the pre-tx channel snapshot for the
    //    response back.
    let verified = verify(provider.inner(), chain_id, payment_payload, requirements).await;
    if !verified.is_valid {
        return failure_from_verify(&network, verified);
    }
    let pre_extra = match verified.extra {
        Some(e) => e,
        None => {
            return BatchSettlementSettleResponse::failure(
                network,
                err::ERR_DEPOSIT_TRANSACTION_FAILED,
            );
        }
    };
    let payer = verified
        .payer
        .clone()
        .unwrap_or_else(|| Address::from(payload.channel_config.payer).to_checksum(None));

    // 2. Build the deposit collector data + collector address.
    let (collector, collector_data) = match &payload.deposit.authorization {
        DepositAuthorization::Erc3009(auth) => (
            ERC3009_DEPOSIT_COLLECTOR_ADDRESS,
            build_erc3009_collector_data(
                auth.valid_after.0,
                auth.valid_before.0,
                auth.salt,
                &auth.signature,
            ),
        ),
        DepositAuthorization::Permit2(auth) => (
            PERMIT2_DEPOSIT_COLLECTOR_ADDRESS,
            // The EIP-2612 gas-sponsoring branch attaches an inline permit
            // segment here. For the baseline production port we forward
            // `0x` and require the payer to already have a Permit2 allowance.
            build_permit2_collector_data(
                auth.nonce.0,
                auth.deadline.0,
                &auth.signature,
                &Bytes::new(),
            ),
        ),
    };

    let Some(deposit_amount_u128) = u256_to_u128(payload.deposit.amount.0) else {
        return BatchSettlementSettleResponse::failure(
            network,
            err::ERR_DEPOSIT_TRANSACTION_FAILED,
        );
    };

    // 3. Submit the deposit transaction.
    let calldata: Bytes = depositCall {
        config: to_abi_channel_config(&payload.channel_config),
        amount: deposit_amount_u128.to::<u128>(),
        collector,
        collectorData: collector_data,
    }
    .abi_encode()
    .into();

    let receipt = match provider
        .send_transaction(MetaTransaction::new(BATCH_SETTLEMENT_ADDRESS, calldata))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let msg: MetaTransactionSendError = e.into();
            return BatchSettlementSettleResponse::failure_with_message(
                network,
                err::ERR_DEPOSIT_TRANSACTION_FAILED,
                msg.to_string(),
            );
        }
    };
    if !receipt.status() {
        return BatchSettlementSettleResponse::failure_with_message(
            network,
            err::ERR_DEPOSIT_TRANSACTION_FAILED,
            format!("transaction reverted (status {})", receipt.status()),
        );
    }
    let tx_hash = receipt.transaction_hash;

    // 4. Build the response. We optimistically project the new channel state
    //    by adding the deposit to the pre-state balance; if the RPC has
    //    already caught up to the new balance, we use the fresh read instead.
    let optimistic_balance = pre_extra.balance.0.saturating_add(deposit_amount_u128);
    let mut extra_state = ChannelStateExtra {
        channel_id: pre_extra.channel_id,
        balance: optimistic_balance.into(),
        total_claimed: pre_extra.total_claimed,
        withdraw_requested_at: pre_extra.withdraw_requested_at,
        refund_nonce: pre_extra.refund_nonce,
        charged_cumulative_amount: None,
    };

    if let Ok(post) = read_channel_state(provider.inner(), pre_extra.channel_id).await
        && post.balance >= optimistic_balance
    {
        extra_state.balance = post.balance.into();
        extra_state.total_claimed = post.total_claimed.into();
        extra_state.withdraw_requested_at = post.withdraw_requested_at;
        extra_state.refund_nonce = post.refund_nonce.into();
    }

    BatchSettlementSettleResponse {
        success: true,
        error_reason: None,
        error_message: None,
        transaction: format!("{tx_hash:#x}"),
        network,
        payer: Some(payer),
        amount: payload.deposit.amount.0.to_string(),
        extra: Some(BatchSettlementSettleExtra {
            channel_state: extra_state,
        }),
    }
}

async fn settle_claim<P>(
    provider: &P,
    chain_id: u64,
    authorizer: Option<&ReceiverAuthorizerSigner>,
    payload: &ClaimPayload,
    requirements: &PaymentRequirements,
) -> BatchSettlementSettleResponse
where
    P: Eip155MetaTransactionProvider + Send + Sync,
    P::Inner: Provider,
    P::Error: Into<MetaTransactionSendError>,
{
    let network = requirements.network.to_string();

    let signature = match resolve_claim_authorizer_signature(payload, authorizer, chain_id) {
        Ok(sig) => sig,
        Err(reason) => return BatchSettlementSettleResponse::failure(network, reason),
    };

    let calls = to_abi_voucher_claims(&payload.claims);
    let calldata = match signature.as_ref() {
        Some(sig_bytes) => claimWithSignatureCall {
            voucherClaims: calls,
            authorizerSignature: sig_bytes.clone(),
        }
        .abi_encode(),
        None => claimCall {
            voucherClaims: calls,
        }
        .abi_encode(),
    };

    submit_to_batch_settlement(
        provider,
        network,
        calldata,
        err::ERR_CLAIM_TRANSACTION_FAILED,
    )
    .await
}

async fn settle_transfer<P>(
    provider: &P,
    payload: &SettlePayload,
    requirements: &PaymentRequirements,
) -> BatchSettlementSettleResponse
where
    P: Eip155MetaTransactionProvider + Send + Sync,
    P::Inner: Provider,
    P::Error: Into<MetaTransactionSendError>,
{
    let network = requirements.network.to_string();
    let calldata = settleCall {
        receiver: payload.receiver.into(),
        token: payload.token.into(),
    }
    .abi_encode();

    let receipt = match provider
        .send_transaction(MetaTransaction::new(
            BATCH_SETTLEMENT_ADDRESS,
            calldata.into(),
        ))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let msg: MetaTransactionSendError = e.into();
            return BatchSettlementSettleResponse::failure_with_message(
                network,
                err::ERR_SETTLE_TRANSACTION_FAILED,
                msg.to_string(),
            );
        }
    };
    if !receipt.status() {
        return BatchSettlementSettleResponse::failure_with_message(
            network,
            err::ERR_SETTLE_TRANSACTION_FAILED,
            format!("transaction reverted (status {})", receipt.status()),
        );
    }
    let tx_hash = receipt.transaction_hash;

    // Pick the `Settled` event amount, if present. Without it, fall back to
    // an empty amount (the upstream impl returns "" for no-op settles).
    let amount = extract_settled_amount(&receipt, payload.receiver.into(), payload.token.into())
        .map(|v| v.to_string())
        .unwrap_or_default();

    BatchSettlementSettleResponse {
        success: true,
        error_reason: None,
        error_message: None,
        transaction: format!("{tx_hash:#x}"),
        network,
        payer: None,
        amount,
        extra: None,
    }
}

async fn settle_refund<P>(
    provider: &P,
    chain_id: u64,
    authorizer: Option<&ReceiverAuthorizerSigner>,
    payload: &EnrichedRefundPayload,
    requirements: &PaymentRequirements,
) -> BatchSettlementSettleResponse
where
    P: Eip155MetaTransactionProvider + Send + Sync,
    P::Inner: Provider,
    P::Error: Into<MetaTransactionSendError>,
{
    let network = requirements.network.to_string();

    let channel_id = compute_channel_id(&payload.channel_config, chain_id);

    let Some(amount_u128) = u256_to_u128(payload.amount.0) else {
        return BatchSettlementSettleResponse::failure(network, err::ERR_REFUND_PAYLOAD);
    };

    let refund_sig = match resolve_refund_authorizer_signature(
        payload,
        authorizer,
        channel_id,
        amount_u128,
        chain_id,
    ) {
        Ok(sig) => sig,
        Err(reason) => return BatchSettlementSettleResponse::failure(network, reason),
    };

    let refund_calldata = refundWithSignatureCall {
        config: to_abi_channel_config(&payload.channel_config),
        amount: amount_u128.to::<u128>(),
        nonce: payload.refund_nonce.0,
        receiverAuthorizerSignature: refund_sig,
    }
    .abi_encode();

    let calldata = if payload.claims.is_empty() {
        refund_calldata
    } else {
        let claim_sig = match resolve_claim_authorizer_signature(
            &ClaimPayload {
                claims: payload.claims.clone(),
                claim_authorizer_signature: payload.claim_authorizer_signature.clone(),
            },
            authorizer,
            chain_id,
        ) {
            Ok(sig) => sig,
            Err(reason) => return BatchSettlementSettleResponse::failure(network, reason),
        };
        let claim_calls = to_abi_voucher_claims(&payload.claims);
        let claim_calldata = match claim_sig {
            Some(sig_bytes) => claimWithSignatureCall {
                voucherClaims: claim_calls,
                authorizerSignature: sig_bytes,
            }
            .abi_encode(),
            None => claimCall {
                voucherClaims: claim_calls,
            }
            .abi_encode(),
        };

        multicallCall {
            data: vec![claim_calldata.into(), refund_calldata.into()],
        }
        .abi_encode()
    };

    let receipt = match provider
        .send_transaction(MetaTransaction::new(
            BATCH_SETTLEMENT_ADDRESS,
            calldata.into(),
        ))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let msg: MetaTransactionSendError = e.into();
            return BatchSettlementSettleResponse::failure_with_message(
                network,
                err::ERR_REFUND_TRANSACTION_FAILED,
                msg.to_string(),
            );
        }
    };
    if !receipt.status() {
        return BatchSettlementSettleResponse::failure_with_message(
            network,
            err::ERR_REFUND_TRANSACTION_FAILED,
            format!("transaction reverted (status {})", receipt.status()),
        );
    }
    let tx_hash = receipt.transaction_hash;

    // Read the post-refund snapshot to surface accurate `balance` / `nonce`.
    let extra = match read_channel_state(provider.inner(), channel_id).await {
        Ok(state) => ChannelStateExtra {
            channel_id,
            balance: state.balance.into(),
            total_claimed: state.total_claimed.into(),
            withdraw_requested_at: state.withdraw_requested_at,
            refund_nonce: state.refund_nonce.into(),
            charged_cumulative_amount: None,
        },
        Err(_) => ChannelStateExtra {
            channel_id,
            balance: U128::ZERO.into(),
            total_claimed: U128::ZERO.into(),
            withdraw_requested_at: 0,
            refund_nonce: (payload.refund_nonce.0 + U256::from(1u64)).into(),
            charged_cumulative_amount: None,
        },
    };

    BatchSettlementSettleResponse {
        success: true,
        error_reason: None,
        error_message: None,
        transaction: format!("{tx_hash:#x}"),
        network,
        payer: Some(Address::from(payload.channel_config.payer).to_checksum(None)),
        amount: payload.amount.0.to_string(),
        extra: Some(BatchSettlementSettleExtra {
            channel_state: extra,
        }),
    }
}

/// Routes the calldata at the `x402BatchSettlement` contract, surfacing
/// receipt status as a canonical error reason on failure.
async fn submit_to_batch_settlement<P>(
    provider: &P,
    network: String,
    calldata: Vec<u8>,
    failure_reason: &'static str,
) -> BatchSettlementSettleResponse
where
    P: Eip155MetaTransactionProvider + Send + Sync,
    P::Inner: Provider,
    P::Error: Into<MetaTransactionSendError>,
{
    let receipt = match provider
        .send_transaction(MetaTransaction::new(
            BATCH_SETTLEMENT_ADDRESS,
            calldata.into(),
        ))
        .await
    {
        Ok(r) => r,
        Err(e) => {
            let msg: MetaTransactionSendError = e.into();
            return BatchSettlementSettleResponse::failure_with_message(
                network,
                failure_reason,
                msg.to_string(),
            );
        }
    };
    if !receipt.status() {
        return BatchSettlementSettleResponse::failure_with_message(
            network,
            failure_reason,
            format!("transaction reverted (status {})", receipt.status()),
        );
    }
    let tx_hash = receipt.transaction_hash;
    BatchSettlementSettleResponse {
        success: true,
        error_reason: None,
        error_message: None,
        transaction: format!("{tx_hash:#x}"),
        network,
        payer: None,
        amount: String::new(),
        extra: None,
    }
}

fn failure_from_verify(
    network: &str,
    verified: super::response::BatchSettlementVerifyResponse,
) -> BatchSettlementSettleResponse {
    let reason = verified
        .invalid_reason
        .as_deref()
        .unwrap_or(err::ERR_DEPOSIT_TRANSACTION_FAILED);
    BatchSettlementSettleResponse {
        success: false,
        error_reason: Some(reason.to_string()),
        error_message: verified.invalid_message.clone(),
        transaction: String::new(),
        network: network.to_string(),
        payer: verified.payer.clone(),
        amount: String::new(),
        extra: None,
    }
}

fn resolve_claim_authorizer_signature(
    payload: &ClaimPayload,
    authorizer: Option<&ReceiverAuthorizerSigner>,
    chain_id: u64,
) -> Result<Option<Bytes>, &'static str> {
    if let Some(existing) = &payload.claim_authorizer_signature {
        return Ok(Some(existing.clone()));
    }
    // No client-supplied signature; sign with the facilitator's authorizer key
    // if every claim row delegates to that address.
    let authorizer = authorizer.ok_or(err::ERR_AUTHORIZER_ADDRESS_MISMATCH)?;
    let want: Address = authorizer.address();
    for claim in &payload.claims {
        if Address::from(claim.voucher.channel.receiver_authorizer) != want {
            return Err(err::ERR_AUTHORIZER_ADDRESS_MISMATCH);
        }
    }
    let sig = authorizer
        .sign_claim_batch(&payload.claims, chain_id)
        .map_err(|_| err::ERR_AUTHORIZER_ADDRESS_MISMATCH)?;
    debug_assert_eq!(
        recover_signer(&sig, compute_claim_batch_digest(&payload.claims, chain_id)),
        Some(want)
    );
    Ok(Some(sig))
}

fn resolve_refund_authorizer_signature(
    payload: &EnrichedRefundPayload,
    authorizer: Option<&ReceiverAuthorizerSigner>,
    channel_id: B256,
    amount: U128,
    chain_id: u64,
) -> Result<Bytes, &'static str> {
    if let Some(existing) = &payload.refund_authorizer_signature {
        return Ok(existing.clone());
    }
    let authorizer = authorizer.ok_or(err::ERR_AUTHORIZER_ADDRESS_MISMATCH)?;
    let want: Address = authorizer.address();
    if Address::from(payload.channel_config.receiver_authorizer) != want {
        return Err(err::ERR_AUTHORIZER_ADDRESS_MISMATCH);
    }
    let sig = authorizer
        .sign_refund(channel_id, amount, payload.refund_nonce.0, chain_id)
        .map_err(|_| err::ERR_AUTHORIZER_ADDRESS_MISMATCH)?;
    debug_assert_eq!(
        recover_signer(
            &sig,
            compute_refund_digest(channel_id, amount, payload.refund_nonce.0, chain_id)
        ),
        Some(want)
    );
    Ok(sig)
}

/// Recovers the EOA signer from a raw 65-byte signature, returning `None`
/// if the bytes don't decode or the recovery fails.
fn recover_signer(signature: &[u8], digest: B256) -> Option<Address> {
    use alloy_primitives::Signature;
    let sig = match signature.len() {
        65 => Signature::from_raw(signature).ok()?.normalized_s(),
        64 => Signature::from_erc2098(signature).normalized_s(),
        _ => return None,
    };
    sig.recover_address_from_prehash(&digest).ok()
}

/// Best-effort extraction of the `amount` field of the `Settled(receiver, token, sender, amount)`
/// event. Returns `None` if the event is not present in the receipt.
fn extract_settled_amount(
    receipt: &alloy_rpc_types_eth::TransactionReceipt,
    receiver: Address,
    token: Address,
) -> Option<U128> {
    use super::abi::X402BatchSettlement::Settled;
    use alloy_sol_types::SolEvent;
    for log in receipt.logs() {
        if log.address() != BATCH_SETTLEMENT_ADDRESS {
            continue;
        }
        let topics = log.topics();
        // `Settled(receiver, token, sender, amount)` is an indexed-3 event.
        if topics.len() != 4 || topics[0] != Settled::SIGNATURE_HASH {
            continue;
        }
        let log_receiver = Address::from_slice(&topics[1].as_slice()[12..]);
        let log_token = Address::from_slice(&topics[2].as_slice()[12..]);
        if log_receiver == receiver
            && log_token == token
            && let Ok(decoded) = Settled::decode_log_data(log.data())
        {
            return Some(U128::from(decoded.amount));
        }
    }
    None
}

fn u256_to_u128(value: U256) -> Option<U128> {
    let bytes: [u8; 32] = value.to_be_bytes();
    if bytes[..16].iter().any(|b| *b != 0) {
        return None;
    }
    let mut narrow = [0u8; 16];
    narrow.copy_from_slice(&bytes[16..]);
    Some(U128::from_be_bytes(narrow))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2_eip155_batch_settlement::types::{
        ChannelConfig as WireChannelConfig, VoucherClaim, VoucherClaimVoucher,
    };
    use alloy_primitives::B256;
    use alloy_signer_local::PrivateKeySigner;

    fn make_config(authorizer: &str) -> WireChannelConfig {
        WireChannelConfig {
            payer: "0x0000000000000000000000000000000000000001"
                .parse()
                .unwrap(),
            payer_authorizer: "0x0000000000000000000000000000000000000002"
                .parse()
                .unwrap(),
            receiver: "0x0000000000000000000000000000000000000003"
                .parse()
                .unwrap(),
            receiver_authorizer: authorizer.parse().unwrap(),
            token: "0x0000000000000000000000000000000000000005"
                .parse()
                .unwrap(),
            withdraw_delay: 900,
            salt: B256::ZERO,
        }
    }

    #[test]
    fn resolve_claim_signature_uses_provided_signature_when_present() {
        let claims = vec![VoucherClaim {
            voucher: VoucherClaimVoucher {
                channel: make_config("0x0000000000000000000000000000000000000004"),
                max_claimable_amount: U128::from(1_000u128).into(),
            },
            signature: Bytes::from_static(&[0x00]),
            total_claimed: U128::from(1_000u128).into(),
        }];
        let payload = ClaimPayload {
            claims,
            claim_authorizer_signature: Some(Bytes::from_static(&[0xab, 0xcd])),
        };
        let sig = resolve_claim_authorizer_signature(&payload, None, 84532).unwrap();
        assert_eq!(sig.as_ref().map(|b| b.as_ref()), Some(&[0xab_u8, 0xcd][..]));
    }

    #[test]
    fn resolve_claim_signature_requires_authorizer_when_signature_missing() {
        let claims = vec![VoucherClaim {
            voucher: VoucherClaimVoucher {
                channel: make_config("0x0000000000000000000000000000000000000004"),
                max_claimable_amount: U128::from(1_000u128).into(),
            },
            signature: Bytes::from_static(&[0x00]),
            total_claimed: U128::from(1_000u128).into(),
        }];
        let payload = ClaimPayload {
            claims,
            claim_authorizer_signature: None,
        };
        let err = resolve_claim_authorizer_signature(&payload, None, 84532).unwrap_err();
        assert_eq!(err, err::ERR_AUTHORIZER_ADDRESS_MISMATCH);
    }

    #[test]
    fn resolve_claim_signature_rejects_mismatched_authorizer() {
        let local = PrivateKeySigner::random();
        let signer = ReceiverAuthorizerSigner::new(local);
        let claims = vec![VoucherClaim {
            voucher: VoucherClaimVoucher {
                channel: make_config("0x0000000000000000000000000000000000000099"), // not the signer
                max_claimable_amount: U128::from(1_000u128).into(),
            },
            signature: Bytes::from_static(&[0x00]),
            total_claimed: U128::from(1_000u128).into(),
        }];
        let payload = ClaimPayload {
            claims,
            claim_authorizer_signature: None,
        };
        let err = resolve_claim_authorizer_signature(&payload, Some(&signer), 84532).unwrap_err();
        assert_eq!(err, err::ERR_AUTHORIZER_ADDRESS_MISMATCH);
    }

    #[test]
    fn resolve_claim_signature_signs_when_authorizer_matches() {
        let local = PrivateKeySigner::random();
        let signer = ReceiverAuthorizerSigner::new(local);
        let authorizer_hex = format!("{:#x}", signer.address());
        let claims = vec![VoucherClaim {
            voucher: VoucherClaimVoucher {
                channel: make_config(&authorizer_hex),
                max_claimable_amount: U128::from(1_000u128).into(),
            },
            signature: Bytes::from_static(&[0x00]),
            total_claimed: U128::from(1_000u128).into(),
        }];
        let payload = ClaimPayload {
            claims: claims.clone(),
            claim_authorizer_signature: None,
        };
        let sig = resolve_claim_authorizer_signature(&payload, Some(&signer), 84532)
            .unwrap()
            .unwrap();
        // The debug_assertion inside the helper already cross-checks the
        // recovered signer; verify the output length here as well.
        assert_eq!(sig.len(), 65);
    }

    #[test]
    fn resolve_refund_signature_passes_through_existing_signature() {
        let payload = EnrichedRefundPayload {
            channel_config: make_config("0x0000000000000000000000000000000000000004"),
            voucher: crate::v2_eip155_batch_settlement::types::VoucherFields {
                channel_id: B256::ZERO,
                max_claimable_amount: U128::ZERO.into(),
                signature: Bytes::new(),
            },
            amount: U256::from(100u64).into(),
            refund_nonce: U256::ZERO.into(),
            claims: Vec::new(),
            refund_authorizer_signature: Some(Bytes::from_static(&[0xde, 0xad])),
            claim_authorizer_signature: None,
        };
        let sig = resolve_refund_authorizer_signature(
            &payload,
            None,
            B256::ZERO,
            U128::from(100u128),
            84532,
        )
        .unwrap();
        assert_eq!(sig.as_ref(), &[0xde, 0xad]);
    }
}
