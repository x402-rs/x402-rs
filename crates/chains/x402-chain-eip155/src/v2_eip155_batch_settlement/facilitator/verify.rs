//! Verify dispatcher for the batch-settlement scheme.
//!
//! Routes incoming verify requests to the right handler based on the payload
//! `type` discriminator: `deposit` / `voucher` / `refund`. Each handler returns
//! a [`BatchSettlementVerifyResponse`] carrying the onchain channel snapshot
//! on success or a canonical error code on failure.

use alloy_primitives::{Address, Bytes, U128, U256};
use alloy_provider::Provider;
use alloy_rpc_types_eth::TransactionRequest;
use alloy_sol_types::{SolCall, SolStruct, eip712_domain};

use super::abi::{
    DepositWitness, IERC20View, PermitWitnessTransferFrom, ReceiveWithAuthorization,
    TokenPermissions, X402BatchSettlement::depositCall,
};
use super::response::{BatchSettlementVerifyExtra, BatchSettlementVerifyResponse};
use super::utils::{
    erc3009_authorization_time_invalid_reason, read_channel_state, to_abi_channel_config,
    validate_channel_config,
};
use super::voucher::{
    VoucherVerifyError, verify_signature_against_signer, verify_voucher_signature,
};
use crate::chain::permit2::PERMIT2_ADDRESS;
use crate::v2_eip155_batch_settlement::constants::{
    BATCH_SETTLEMENT_ADDRESS, ERC3009_DEPOSIT_COLLECTOR_ADDRESS, PERMIT2_DEPOSIT_COLLECTOR_ADDRESS,
};
use crate::v2_eip155_batch_settlement::encoding::{
    build_erc3009_collector_data, build_erc3009_deposit_nonce, build_permit2_collector_data,
};
use crate::v2_eip155_batch_settlement::errors as err;
use crate::v2_eip155_batch_settlement::types::{
    AssetTransferMethod, BatchSettlementPayload, BatchSettlementRefundPayload, ChannelConfig,
    DepositAuthorization, DepositPayload, Erc3009Authorization, PaymentPayload,
    PaymentRequirements, Permit2Authorization, RefundPayload, VoucherFields, VoucherPayload,
};

/// Top-level dispatcher.
pub async fn verify<P>(
    provider: &P,
    chain_id: u64,
    payment_payload: &PaymentPayload,
    requirements: &PaymentRequirements,
) -> BatchSettlementVerifyResponse
where
    P: Provider,
{
    if payment_payload.accepted.network != requirements.network {
        return BatchSettlementVerifyResponse::invalid(None, err::ERR_NETWORK_MISMATCH);
    }

    match &payment_payload.payload {
        BatchSettlementPayload::Deposit(deposit) => {
            verify_deposit(provider, chain_id, deposit, requirements).await
        }
        BatchSettlementPayload::Voucher(voucher_payload) => {
            verify_voucher_payload(provider, chain_id, voucher_payload, requirements).await
        }
        BatchSettlementPayload::Refund(BatchSettlementRefundPayload::Client(refund)) => {
            verify_refund(provider, chain_id, refund, requirements).await
        }
        BatchSettlementPayload::Refund(BatchSettlementRefundPayload::Enriched(_)) => {
            BatchSettlementVerifyResponse::invalid(None, err::ERR_INVALID_PAYLOAD_TYPE)
        }
        BatchSettlementPayload::Claim(_) | BatchSettlementPayload::Settle(_) => {
            BatchSettlementVerifyResponse::invalid(None, err::ERR_INVALID_PAYLOAD_TYPE)
        }
    }
}

async fn verify_voucher_payload<P>(
    provider: &P,
    chain_id: u64,
    payload: &VoucherPayload,
    requirements: &PaymentRequirements,
) -> BatchSettlementVerifyResponse
where
    P: Provider,
{
    verify_voucher_or_refund(
        provider,
        chain_id,
        &payload.channel_config,
        &payload.voucher,
        requirements,
        /* is_refund = */ false,
    )
    .await
}

async fn verify_refund<P>(
    provider: &P,
    chain_id: u64,
    refund: &RefundPayload,
    requirements: &PaymentRequirements,
) -> BatchSettlementVerifyResponse
where
    P: Provider,
{
    verify_voucher_or_refund(
        provider,
        chain_id,
        &refund.channel_config,
        &refund.voucher,
        requirements,
        /* is_refund = */ true,
    )
    .await
}

async fn verify_voucher_or_refund<P>(
    provider: &P,
    chain_id: u64,
    channel_config: &ChannelConfig,
    voucher: &VoucherFields,
    requirements: &PaymentRequirements,
    is_refund: bool,
) -> BatchSettlementVerifyResponse
where
    P: Provider,
{
    let payer_addr: Address = channel_config.payer.into();
    let payer = payer_addr.to_checksum(None);

    if let Err(reason) =
        validate_channel_config(channel_config, voucher.channel_id, requirements, chain_id)
    {
        return BatchSettlementVerifyResponse::invalid(Some(payer), reason);
    }

    if let Err(e) = verify_voucher_signature(
        provider,
        voucher,
        payer_addr,
        channel_config.payer_authorizer.into(),
        chain_id,
    )
    .await
    {
        return BatchSettlementVerifyResponse::invalid(Some(payer), e.as_error_code());
    }

    let state = match read_channel_state(provider, voucher.channel_id).await {
        Ok(state) => state,
        Err(reason) => return BatchSettlementVerifyResponse::invalid(Some(payer), reason),
    };

    if state.is_empty() {
        return BatchSettlementVerifyResponse::invalid(Some(payer), err::ERR_CHANNEL_NOT_FOUND);
    }

    let max_claimable = voucher.max_claimable_amount.0;
    if max_claimable > state.balance {
        return BatchSettlementVerifyResponse::invalid(
            Some(payer),
            err::ERR_CUMULATIVE_EXCEEDS_BALANCE,
        );
    }

    // Spec rule 10 for voucher verification: refund vouchers may equal
    // `totalClaimed` (zero-charge); ordinary voucher payments must strictly
    // exceed it. Deposit verification is handled separately below and follows
    // the Go reference by allowing equality for balance-only top-ups.
    let below_claimed = if is_refund {
        max_claimable < state.total_claimed
    } else {
        max_claimable <= state.total_claimed
    };
    if below_claimed {
        return BatchSettlementVerifyResponse::invalid(
            Some(payer),
            err::ERR_CUMULATIVE_AMOUNT_BELOW_CLAIMED,
        );
    }

    BatchSettlementVerifyResponse::valid(
        payer,
        BatchSettlementVerifyExtra::from_state(voucher.channel_id, &state),
    )
}

async fn verify_deposit<P>(
    provider: &P,
    chain_id: u64,
    payload: &DepositPayload,
    requirements: &PaymentRequirements,
) -> BatchSettlementVerifyResponse
where
    P: Provider,
{
    let payer_addr: Address = payload.channel_config.payer.into();
    let payer = payer_addr.to_checksum(None);

    if let Err(reason) = validate_channel_config(
        &payload.channel_config,
        payload.voucher.channel_id,
        requirements,
        chain_id,
    ) {
        return BatchSettlementVerifyResponse::invalid(Some(payer), reason);
    }

    let deposit_amount_u128 = match deposit_amount_to_u128(payload.deposit.amount.0) {
        Ok(amount) => amount,
        Err(reason) => return BatchSettlementVerifyResponse::invalid(Some(payer), reason),
    };

    if let Err(reason) =
        verify_deposit_authorization(provider, payload, requirements, chain_id).await
    {
        return BatchSettlementVerifyResponse::invalid(Some(payer), reason);
    }

    if let Err(e) = verify_voucher_signature(
        provider,
        &payload.voucher,
        payer_addr,
        payload.channel_config.payer_authorizer.into(),
        chain_id,
    )
    .await
    {
        return BatchSettlementVerifyResponse::invalid(Some(payer), e.as_error_code());
    }

    let state = match read_channel_state(provider, payload.voucher.channel_id).await {
        Ok(state) => state,
        Err(reason) => return BatchSettlementVerifyResponse::invalid(Some(payer), reason),
    };

    let asset_addr: Address = requirements.asset.into();
    let token_contract = IERC20View::new(asset_addr, provider);
    let payer_balance = match token_contract.balanceOf(payer_addr).call().await {
        Ok(b) => b,
        Err(_) => {
            return BatchSettlementVerifyResponse::invalid(Some(payer), err::ERR_RPC_READ_FAILED);
        }
    };
    if payer_balance < payload.deposit.amount.0 {
        return BatchSettlementVerifyResponse::invalid(Some(payer), err::ERR_INSUFFICIENT_BALANCE);
    }

    let effective_balance = state.balance.saturating_add(deposit_amount_u128);

    let max_claimable = payload.voucher.max_claimable_amount.0;
    if max_claimable > effective_balance {
        return BatchSettlementVerifyResponse::invalid(
            Some(payer),
            err::ERR_CUMULATIVE_EXCEEDS_BALANCE,
        );
    }
    // Deposit top-ups may carry the current cumulative voucher without
    // charging a new request. Match the Go reference: reject only values below
    // the already-claimed total, not equality.
    if max_claimable < state.total_claimed {
        return BatchSettlementVerifyResponse::invalid(
            Some(payer),
            err::ERR_CUMULATIVE_AMOUNT_BELOW_CLAIMED,
        );
    }

    let calldata = build_deposit_calldata(payload, deposit_amount_u128);
    if let Err(message) = simulate_batch_settlement_call(provider, calldata).await {
        return BatchSettlementVerifyResponse::invalid_with_message(
            Some(payer),
            err::ERR_DEPOSIT_SIMULATION_FAILED,
            message,
        );
    }

    BatchSettlementVerifyResponse::valid(
        payer,
        BatchSettlementVerifyExtra {
            channel_id: payload.voucher.channel_id,
            balance: effective_balance.into(),
            total_claimed: state.total_claimed.into(),
            withdraw_requested_at: state.withdraw_requested_at,
            refund_nonce: state.refund_nonce.into(),
        },
    )
}

async fn verify_deposit_authorization<P>(
    provider: &P,
    payload: &DepositPayload,
    requirements: &PaymentRequirements,
    chain_id: u64,
) -> Result<(), &'static str>
where
    P: Provider,
{
    match &payload.deposit.authorization {
        DepositAuthorization::Erc3009(auth) => {
            if requirements.extra.asset_transfer_method == Some(AssetTransferMethod::Permit2) {
                return Err(err::ERR_PERMIT2_AUTHORIZATION_REQUIRED);
            }
            verify_erc3009_authorization(provider, payload, auth, requirements, chain_id).await
        }
        DepositAuthorization::Permit2(auth) => {
            if requirements.extra.asset_transfer_method == Some(AssetTransferMethod::Eip3009) {
                return Err(err::ERR_ERC3009_AUTHORIZATION_REQUIRED);
            }
            verify_permit2_authorization(provider, payload, auth, requirements, chain_id).await
        }
    }
}

async fn verify_erc3009_authorization<P>(
    provider: &P,
    payload: &DepositPayload,
    auth: &Erc3009Authorization,
    requirements: &PaymentRequirements,
    chain_id: u64,
) -> Result<(), &'static str>
where
    P: Provider,
{
    if requirements.extra.name.is_empty() || requirements.extra.version.is_empty() {
        return Err(err::ERR_MISSING_EIP712_DOMAIN);
    }

    let valid_after = auth.valid_after.0;
    let valid_before = auth.valid_before.0;
    if let Some(reason) = erc3009_authorization_time_invalid_reason(valid_after, valid_before) {
        return Err(reason);
    }

    // The nonce binds the ERC-3009 authorization to (channelId, salt): two
    // deposits to the same channel must use distinct salts to avoid nonce
    // collisions onchain.
    let erc3009_nonce = build_erc3009_deposit_nonce(payload.voucher.channel_id, auth.salt);

    let token_addr: Address = requirements.asset.into();
    let payer_addr: Address = payload.channel_config.payer.into();
    let receive_auth = ReceiveWithAuthorization {
        from: payer_addr,
        to: ERC3009_DEPOSIT_COLLECTOR_ADDRESS,
        value: payload.deposit.amount.0,
        validAfter: valid_after,
        validBefore: valid_before,
        nonce: erc3009_nonce,
    };
    let domain = eip712_domain! {
        name: requirements.extra.name.clone(),
        version: requirements.extra.version.clone(),
        chain_id: chain_id,
        verifying_contract: token_addr,
    };
    let digest = receive_auth.eip712_signing_hash(&domain);

    match verify_token_authorization_signature(
        provider,
        &auth.signature,
        &digest,
        &payload.channel_config,
    )
    .await
    {
        Ok(()) => Ok(()),
        Err(e) => Err(erc3009_authorization_signature_error_reason(e)),
    }
}

async fn verify_permit2_authorization<P>(
    provider: &P,
    payload: &DepositPayload,
    auth: &Permit2Authorization,
    requirements: &PaymentRequirements,
    chain_id: u64,
) -> Result<(), &'static str>
where
    P: Provider,
{
    let payer: Address = payload.channel_config.payer.into();
    if Address::from(auth.from) != payer {
        return Err(err::ERR_DEPOSIT_PAYLOAD);
    }
    if Address::from(auth.spender) != PERMIT2_DEPOSIT_COLLECTOR_ADDRESS {
        return Err(err::ERR_PERMIT2_INVALID_SPENDER);
    }
    let asset: Address = requirements.asset.into();
    if Address::from(auth.permitted.token) != asset {
        return Err(err::ERR_TOKEN_MISMATCH);
    }
    // The Permit2 collector pulls exactly `deposit.amount`; require the
    // signed permission to bind that same amount rather than a wider cap.
    if auth.permitted.amount.0 != payload.deposit.amount.0 {
        return Err(err::ERR_PERMIT2_AMOUNT_MISMATCH);
    }
    if auth.witness.channel_id != payload.voucher.channel_id {
        return Err(err::ERR_CHANNEL_ID_MISMATCH);
    }
    let now = x402_types::timestamp::UnixTimestamp::now().as_secs();
    let now_u256 = U256::from(now);
    if auth.deadline.0 < now_u256 + U256::from(6u64) {
        return Err(err::ERR_PERMIT2_DEADLINE_EXPIRED);
    }

    let permit = PermitWitnessTransferFrom {
        permitted: TokenPermissions {
            token: asset,
            amount: payload.deposit.amount.0,
        },
        spender: auth.spender.into(),
        nonce: auth.nonce.0,
        deadline: auth.deadline.0,
        witness: DepositWitness {
            channelId: payload.voucher.channel_id,
        },
    };
    let domain = eip712_domain! {
        name: "Permit2",
        chain_id: chain_id,
        verifying_contract: PERMIT2_ADDRESS,
    };
    let digest = permit.eip712_signing_hash(&domain);
    verify_token_authorization_signature(
        provider,
        &auth.signature,
        &digest,
        &payload.channel_config,
    )
    .await
    .map_err(permit2_authorization_signature_error_reason)?;

    let token_contract = IERC20View::new(asset, provider);
    let allowance = token_contract
        .allowance(payer, PERMIT2_ADDRESS)
        .call()
        .await
        .map_err(|_| err::ERR_RPC_READ_FAILED)?;
    if allowance < payload.deposit.amount.0 {
        return Err(err::ERR_PERMIT2_ALLOWANCE_REQUIRED);
    }

    Ok(())
}

async fn verify_token_authorization_signature<P>(
    provider: &P,
    signature: &[u8],
    digest: &alloy_primitives::B256,
    channel_config: &ChannelConfig,
) -> Result<(), VoucherVerifyError>
where
    P: Provider,
{
    let payer: Address = channel_config.payer.into();
    let payer_authorizer: Address = channel_config.payer_authorizer.into();
    if payer_authorizer == Address::ZERO {
        // `payerAuthorizer == address(0)` is the scheme's smart-wallet mode:
        // vouchers and token-transfer authorizations are validated through
        // EIP-1271 against the payer contract.
        return verify_signature_against_signer(provider, signature, digest, payer, Address::ZERO)
            .await;
    }

    // Permit2 and ERC-3009 token-transfer authorizations are signed by the
    // token owner (`payer`), not the channel's voucher authorizer.
    let ecdsa_result =
        verify_signature_against_signer(provider, signature, digest, payer, payer).await;
    if ecdsa_result.is_ok() {
        return ecdsa_result;
    }

    // Hybrid mode: the payer can still be a smart-wallet contract while a
    // separate non-zero payerAuthorizer signs vouchers. In that case the token
    // authorization itself is validated by the payer contract via EIP-1271.
    let code = provider
        .get_code_at(payer)
        .into_future()
        .await
        .map_err(|_| VoucherVerifyError::RpcReadFailed)?;
    if code.is_empty() {
        return ecdsa_result;
    }

    verify_signature_against_signer(provider, signature, digest, payer, Address::ZERO).await
}

fn erc3009_authorization_signature_error_reason(error: VoucherVerifyError) -> &'static str {
    match error {
        VoucherVerifyError::RpcReadFailed => err::ERR_RPC_READ_FAILED,
        VoucherVerifyError::InvalidFormat | VoucherVerifyError::InvalidSignature => {
            err::ERR_INVALID_RECEIVE_AUTHORIZATION_SIGNATURE
        }
    }
}

fn permit2_authorization_signature_error_reason(error: VoucherVerifyError) -> &'static str {
    match error {
        VoucherVerifyError::RpcReadFailed => err::ERR_RPC_READ_FAILED,
        VoucherVerifyError::InvalidFormat | VoucherVerifyError::InvalidSignature => {
            err::ERR_PERMIT2_INVALID_SIGNATURE
        }
    }
}

fn build_deposit_calldata(payload: &DepositPayload, deposit_amount: U128) -> Bytes {
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
            build_permit2_collector_data(
                auth.nonce.0,
                auth.deadline.0,
                &auth.signature,
                &Bytes::new(),
            ),
        ),
    };

    depositCall {
        config: to_abi_channel_config(&payload.channel_config),
        amount: deposit_amount.to::<u128>(),
        collector,
        collectorData: collector_data,
    }
    .abi_encode()
    .into()
}

async fn simulate_batch_settlement_call<P>(provider: &P, calldata: Bytes) -> Result<(), String>
where
    P: Provider,
{
    let request = TransactionRequest::default()
        .input(calldata.into())
        .to(BATCH_SETTLEMENT_ADDRESS);
    provider
        .call(request)
        .await
        .map(|_| ())
        .map_err(|e| e.to_string())
}

/// Converts a `U256` to `U128`, returning `None` when the value overflows.
fn u256_to_u128(value: U256) -> Option<U128> {
    let bytes: [u8; 32] = value.to_be_bytes();
    if bytes[..16].iter().any(|b| *b != 0) {
        return None;
    }
    let mut narrow = [0u8; 16];
    narrow.copy_from_slice(&bytes[16..]);
    Some(U128::from_be_bytes(narrow))
}

fn deposit_amount_to_u128(value: U256) -> Result<U128, &'static str> {
    if value == U256::ZERO {
        return Err(err::ERR_DEPOSIT_PAYLOAD);
    }
    u256_to_u128(value).ok_or(err::ERR_CUMULATIVE_EXCEEDS_BALANCE)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u256_to_u128_round_trips_small_values() {
        assert_eq!(u256_to_u128(U256::ZERO), Some(U128::ZERO));
        assert_eq!(
            u256_to_u128(U256::from(1_000u64)),
            Some(U128::from(1_000u128))
        );
        assert_eq!(
            u256_to_u128(U256::from(u128::MAX)),
            Some(U128::from(u128::MAX))
        );
    }

    #[test]
    fn u256_to_u128_returns_none_on_overflow() {
        let overflowed = U256::from(u128::MAX) + U256::from(1u64);
        assert_eq!(u256_to_u128(overflowed), None);
    }

    #[test]
    fn deposit_amount_to_u128_rejects_zero() {
        assert_eq!(
            deposit_amount_to_u128(U256::ZERO),
            Err(err::ERR_DEPOSIT_PAYLOAD)
        );
    }

    #[test]
    fn deposit_amount_to_u128_rejects_overflow() {
        let overflowed = U256::from(u128::MAX) + U256::from(1u64);
        assert_eq!(
            deposit_amount_to_u128(overflowed),
            Err(err::ERR_CUMULATIVE_EXCEEDS_BALANCE)
        );
    }

    #[test]
    fn token_authorization_error_mapping_preserves_rpc_errors() {
        assert_eq!(
            erc3009_authorization_signature_error_reason(VoucherVerifyError::RpcReadFailed),
            err::ERR_RPC_READ_FAILED
        );
        assert_eq!(
            permit2_authorization_signature_error_reason(VoucherVerifyError::RpcReadFailed),
            err::ERR_RPC_READ_FAILED
        );
    }

    #[test]
    fn token_authorization_error_mapping_preserves_signature_errors() {
        assert_eq!(
            erc3009_authorization_signature_error_reason(VoucherVerifyError::InvalidSignature),
            err::ERR_INVALID_RECEIVE_AUTHORIZATION_SIGNATURE
        );
        assert_eq!(
            permit2_authorization_signature_error_reason(VoucherVerifyError::InvalidFormat),
            err::ERR_PERMIT2_INVALID_SIGNATURE
        );
    }
}
