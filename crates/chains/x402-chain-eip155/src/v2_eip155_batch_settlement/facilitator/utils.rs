//! Shared helpers for batch-settlement verify / settle flows.
//!
//! Mirrors `typescript/packages/mechanisms/evm/src/batch-settlement/facilitator/utils.ts`.

use alloy_primitives::{Address, B256, U128, U256};
use alloy_provider::Provider;
use alloy_sol_types::{Eip712Domain, SolStruct, eip712_domain};
use x402_types::timestamp::UnixTimestamp;

use super::abi::{ChannelConfig as AbiChannelConfig, Voucher as AbiVoucher, X402BatchSettlement};
use crate::v2_eip155_batch_settlement::constants::{
    BATCH_SETTLEMENT_ADDRESS, BATCH_SETTLEMENT_DOMAIN_NAME, BATCH_SETTLEMENT_DOMAIN_VERSION,
    MAX_WITHDRAW_DELAY, MIN_WITHDRAW_DELAY,
};
use crate::v2_eip155_batch_settlement::errors as err;
use crate::v2_eip155_batch_settlement::types::{
    BatchSettlementPaymentRequirementsExtra, ChannelConfig as WireChannelConfig,
    PaymentRequirements,
};

/// Onchain channel snapshot read via three view calls on the
/// `x402BatchSettlement` contract.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OnchainChannelState {
    pub balance: U128,
    pub total_claimed: U128,
    pub withdraw_requested_amount: U128,
    pub withdraw_requested_at: u64,
    pub refund_nonce: U256,
}

impl OnchainChannelState {
    /// True when the channel has never been funded (balance == 0).
    pub fn is_empty(&self) -> bool {
        self.balance == U128::ZERO
    }
}

/// Builds the chain-bound EIP-712 domain used by every batch-settlement signed
/// structure (`Voucher`, `Refund`, `ClaimBatch`, `ChannelConfig`).
pub fn batch_settlement_domain(chain_id: u64) -> Eip712Domain {
    eip712_domain! {
        name: BATCH_SETTLEMENT_DOMAIN_NAME,
        version: BATCH_SETTLEMENT_DOMAIN_VERSION,
        chain_id: chain_id,
        verifying_contract: BATCH_SETTLEMENT_ADDRESS,
    }
}

/// Converts the wire-format `ChannelConfig` into the ABI tuple expected by the
/// `x402BatchSettlement` contract calls.
///
/// `withdrawDelay` is encoded as `uint40`. Callers validate the value lives
/// in `[MIN_WITHDRAW_DELAY, MAX_WITHDRAW_DELAY]` before reaching here, so the
/// `TryFrom<u64>` conversion is infallible in practice; a saturating fallback
/// preserves the type signature.
pub fn to_abi_channel_config(config: &WireChannelConfig) -> AbiChannelConfig {
    let withdraw_delay = alloy_primitives::Uint::<40, 1>::try_from(config.withdraw_delay)
        .unwrap_or_else(|_| alloy_primitives::Uint::<40, 1>::from(MAX_WITHDRAW_DELAY));
    AbiChannelConfig {
        payer: config.payer.into(),
        payerAuthorizer: config.payer_authorizer.into(),
        receiver: config.receiver.into(),
        receiverAuthorizer: config.receiver_authorizer.into(),
        token: config.token.into(),
        withdrawDelay: withdraw_delay,
        salt: config.salt,
    }
}

/// Computes the chain-bound channel id from a [`WireChannelConfig`].
///
/// `channelId = EIP712Hash(ChannelConfig)` under the `x402 Batch Settlement`
/// domain. The domain binds the hash to the EVM `chainId` and the deployed
/// `x402BatchSettlement` address, so the same config produces different
/// channel ids across chains or deployments.
pub fn compute_channel_id(config: &WireChannelConfig, chain_id: u64) -> B256 {
    let domain = batch_settlement_domain(chain_id);
    let abi = to_abi_channel_config(config);
    abi.eip712_signing_hash(&domain)
}

/// Computes the EIP-712 digest a voucher signer commits to.
///
/// `maxClaimableAmount` is encoded as `uint128`; `alloy_sol_types::sol!`
/// emits a native `u128` for that field, so we narrow the wrapped
/// [`U128`] via `to::<u128>()` (lossless: both have 128-bit width).
pub fn compute_voucher_digest(channel_id: B256, max_claimable_amount: U128, chain_id: u64) -> B256 {
    let domain = batch_settlement_domain(chain_id);
    AbiVoucher {
        channelId: channel_id,
        maxClaimableAmount: max_claimable_amount.to::<u128>(),
    }
    .eip712_signing_hash(&domain)
}

/// Validates that a [`WireChannelConfig`] is consistent with the claimed
/// `channel_id` and the server's [`PaymentRequirements`].
///
/// Returns an error code string on mismatch, matching the spec's error
/// taxonomy. Returns `Ok(())` when the config is well-formed.
pub fn validate_channel_config(
    config: &WireChannelConfig,
    channel_id: B256,
    requirements: &PaymentRequirements,
    chain_id: u64,
) -> Result<(), &'static str> {
    let computed_id = compute_channel_id(config, chain_id);
    if computed_id != channel_id {
        return Err(err::ERR_CHANNEL_ID_MISMATCH);
    }

    let pay_to: Address = requirements.pay_to.into();
    if Address::from(config.receiver) != pay_to {
        return Err(err::ERR_RECEIVER_MISMATCH);
    }

    let required_authorizer: Address = requirements.extra.receiver_authorizer.into();
    if required_authorizer == Address::ZERO
        || Address::from(config.receiver_authorizer) != required_authorizer
    {
        return Err(err::ERR_RECEIVER_AUTHORIZER_MISMATCH);
    }

    let asset: Address = requirements.asset.into();
    if Address::from(config.token) != asset {
        return Err(err::ERR_TOKEN_MISMATCH);
    }

    if config.withdraw_delay != requirements.extra.withdraw_delay {
        return Err(err::ERR_WITHDRAW_DELAY_MISMATCH);
    }

    if config.withdraw_delay < MIN_WITHDRAW_DELAY || config.withdraw_delay > MAX_WITHDRAW_DELAY {
        return Err(err::ERR_WITHDRAW_DELAY_OUT_OF_RANGE);
    }

    Ok(())
}

/// Time-window check for an ERC-3009 `ReceiveWithAuthorization`, matching the
/// reference implementations: a 6-second grace window prevents racy expiries.
pub fn erc3009_authorization_time_invalid_reason(
    valid_after: U256,
    valid_before: U256,
) -> Option<&'static str> {
    let now = UnixTimestamp::now().as_secs();
    let now_u256 = U256::from(now);
    if valid_before < now_u256 + U256::from(6u64) {
        return Some(err::ERR_VALID_BEFORE_EXPIRED);
    }
    if valid_after > now_u256 {
        return Some(err::ERR_VALID_AFTER_IN_FUTURE);
    }
    None
}

/// Sanity check that the requirements and the payload's `accepted` block agree
/// on scheme and network. Returns an error code string on mismatch.
pub fn assert_scheme_and_network<T>(
    accepted: &x402_types::proto::v2::PaymentRequirements<
        crate::v2_eip155_batch_settlement::types::BatchSettlementScheme,
        crate::v2_eip155_batch_settlement::types::U256String,
        crate::chain::ChecksummedAddress,
        BatchSettlementPaymentRequirementsExtra,
    >,
    requirements: &PaymentRequirements,
    _phantom: std::marker::PhantomData<T>,
) -> Result<(), &'static str> {
    // The literal scheme types are unit structs constrained to "batch-settlement";
    // a deserialization-level mismatch would already have been rejected. We
    // still cross-check the network reference between `accepted` and the
    // server's requirements to prevent cross-chain payload reuse.
    if accepted.network != requirements.network {
        return Err(err::ERR_NETWORK_MISMATCH);
    }
    Ok(())
}

/// Reads the onchain channel snapshot via `channels`, `pendingWithdrawals`,
/// and `refundNonce`. The three calls are issued concurrently against the
/// provider; a failure in any of them is mapped to `ERR_RPC_READ_FAILED`.
pub async fn read_channel_state<P>(
    provider: &P,
    channel_id: B256,
) -> Result<OnchainChannelState, &'static str>
where
    P: Provider,
{
    let contract = X402BatchSettlement::new(BATCH_SETTLEMENT_ADDRESS, provider);
    let channels_call = contract.channels(channel_id);
    let pending_call = contract.pendingWithdrawals(channel_id);
    let nonce_call = contract.refundNonce(channel_id);

    let (channels_res, pending_res, nonce_res) =
        tokio::join!(channels_call.call(), pending_call.call(), nonce_call.call());

    let channels = channels_res.map_err(|_| err::ERR_RPC_READ_FAILED)?;
    let pending = pending_res.map_err(|_| err::ERR_RPC_READ_FAILED)?;
    let refund_nonce = nonce_res.map_err(|_| err::ERR_RPC_READ_FAILED)?;

    Ok(OnchainChannelState {
        balance: U128::from(channels.balance),
        total_claimed: U128::from(channels.totalClaimed),
        withdraw_requested_amount: U128::from(pending.amount),
        // `initiatedAt` is `uint40` → `Uint<40, 1>`; truncating to `u64` is
        // lossless because `2^40 - 1 < u64::MAX`.
        withdraw_requested_at: pending.initiatedAt.to::<u64>(),
        refund_nonce,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2_eip155_batch_settlement::types::{
        BatchSettlementPaymentRequirementsExtra, U256String,
    };
    use alloy_primitives::U256;
    use x402_types::chain::ChainId;

    fn make_requirements(
        receiver: &str,
        receiver_authorizer: &str,
        token: &str,
        withdraw_delay: u64,
    ) -> PaymentRequirements {
        PaymentRequirements {
            scheme: crate::v2_eip155_batch_settlement::types::BatchSettlementScheme,
            network: ChainId::new("eip155", "84532"),
            amount: U256String::from(U256::from(1_000_000u64)),
            pay_to: receiver.parse().unwrap(),
            max_timeout_seconds: 300,
            asset: token.parse().unwrap(),
            extra: BatchSettlementPaymentRequirementsExtra {
                receiver_authorizer: receiver_authorizer.parse().unwrap(),
                withdraw_delay,
                name: "USDC".into(),
                version: "2".into(),
                asset_transfer_method: None,
                channel_state: None,
                voucher_state: None,
            },
        }
    }

    fn make_config(
        payer: &str,
        payer_authorizer: &str,
        receiver: &str,
        receiver_authorizer: &str,
        token: &str,
        withdraw_delay: u64,
    ) -> WireChannelConfig {
        WireChannelConfig {
            payer: payer.parse().unwrap(),
            payer_authorizer: payer_authorizer.parse().unwrap(),
            receiver: receiver.parse().unwrap(),
            receiver_authorizer: receiver_authorizer.parse().unwrap(),
            token: token.parse().unwrap(),
            withdraw_delay,
            salt: B256::ZERO,
        }
    }

    #[test]
    fn channel_id_is_chain_bound() {
        let cfg = make_config(
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            900,
        );
        let id_base = compute_channel_id(&cfg, 8453);
        let id_optimism = compute_channel_id(&cfg, 10);
        assert_ne!(
            id_base, id_optimism,
            "same config on different chains must hash to different ids"
        );
    }

    #[test]
    fn channel_id_changes_with_salt() {
        let mut cfg = make_config(
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            900,
        );
        let id1 = compute_channel_id(&cfg, 8453);
        cfg.salt = B256::repeat_byte(0x11);
        let id2 = compute_channel_id(&cfg, 8453);
        assert_ne!(id1, id2);
    }

    #[test]
    fn channel_id_changes_when_authorizer_changes() {
        let cfg_a = make_config(
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            900,
        );
        let cfg_b = make_config(
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000099", // different payerAuthorizer
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            900,
        );
        let id_a = compute_channel_id(&cfg_a, 8453);
        let id_b = compute_channel_id(&cfg_b, 8453);
        assert_ne!(id_a, id_b);
    }

    #[test]
    fn validate_channel_config_accepts_aligned() {
        let cfg = make_config(
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            900,
        );
        let req = make_requirements(
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            900,
        );
        let id = compute_channel_id(&cfg, 84532);
        assert_eq!(validate_channel_config(&cfg, id, &req, 84532), Ok(()));
    }

    #[test]
    fn validate_channel_config_detects_channel_id_mismatch() {
        let cfg = make_config(
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            900,
        );
        let req = make_requirements(
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            900,
        );
        let bogus = B256::repeat_byte(0xff);
        assert_eq!(
            validate_channel_config(&cfg, bogus, &req, 84532),
            Err(err::ERR_CHANNEL_ID_MISMATCH)
        );
    }

    #[test]
    fn validate_channel_config_detects_receiver_mismatch() {
        let cfg = make_config(
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            "0x0000000000000000000000000000000000000099",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            900,
        );
        let req = make_requirements(
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            900,
        );
        let id = compute_channel_id(&cfg, 84532);
        assert_eq!(
            validate_channel_config(&cfg, id, &req, 84532),
            Err(err::ERR_RECEIVER_MISMATCH)
        );
    }

    #[test]
    fn validate_channel_config_detects_zero_authorizer() {
        let cfg = make_config(
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000000",
            "0x0000000000000000000000000000000000000005",
            900,
        );
        let req = make_requirements(
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000000",
            "0x0000000000000000000000000000000000000005",
            900,
        );
        let id = compute_channel_id(&cfg, 84532);
        // Zero `receiver_authorizer` in requirements is treated as missing.
        assert_eq!(
            validate_channel_config(&cfg, id, &req, 84532),
            Err(err::ERR_RECEIVER_AUTHORIZER_MISMATCH)
        );
    }

    #[test]
    fn validate_channel_config_detects_token_mismatch() {
        let cfg = make_config(
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000099",
            900,
        );
        let req = make_requirements(
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            900,
        );
        let id = compute_channel_id(&cfg, 84532);
        assert_eq!(
            validate_channel_config(&cfg, id, &req, 84532),
            Err(err::ERR_TOKEN_MISMATCH)
        );
    }

    #[test]
    fn validate_channel_config_detects_withdraw_delay_mismatch() {
        let cfg = make_config(
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            1_800,
        );
        let req = make_requirements(
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            900,
        );
        let id = compute_channel_id(&cfg, 84532);
        assert_eq!(
            validate_channel_config(&cfg, id, &req, 84532),
            Err(err::ERR_WITHDRAW_DELAY_MISMATCH)
        );
    }

    #[test]
    fn validate_channel_config_detects_withdraw_delay_out_of_range() {
        let cfg = make_config(
            "0x0000000000000000000000000000000000000001",
            "0x0000000000000000000000000000000000000002",
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            60, // below MIN_WITHDRAW_DELAY (900 s)
        );
        let req = make_requirements(
            "0x0000000000000000000000000000000000000003",
            "0x0000000000000000000000000000000000000004",
            "0x0000000000000000000000000000000000000005",
            60,
        );
        let id = compute_channel_id(&cfg, 84532);
        assert_eq!(
            validate_channel_config(&cfg, id, &req, 84532),
            Err(err::ERR_WITHDRAW_DELAY_OUT_OF_RANGE)
        );
    }

    #[test]
    fn erc3009_authorization_time_check_expired() {
        let now = UnixTimestamp::now().as_secs();
        let reason = erc3009_authorization_time_invalid_reason(
            U256::ZERO,
            U256::from(now.saturating_sub(1)),
        );
        assert_eq!(reason, Some(err::ERR_VALID_BEFORE_EXPIRED));
    }

    #[test]
    fn erc3009_authorization_time_check_future_valid_after() {
        let now = UnixTimestamp::now().as_secs();
        let reason = erc3009_authorization_time_invalid_reason(
            U256::from(now + 600),
            U256::from(now + 1_000),
        );
        assert_eq!(reason, Some(err::ERR_VALID_AFTER_IN_FUTURE));
    }

    #[test]
    fn erc3009_authorization_time_check_ok() {
        let now = UnixTimestamp::now().as_secs();
        let reason = erc3009_authorization_time_invalid_reason(
            U256::from(now.saturating_sub(10)),
            U256::from(now + 600),
        );
        assert_eq!(reason, None);
    }
}
