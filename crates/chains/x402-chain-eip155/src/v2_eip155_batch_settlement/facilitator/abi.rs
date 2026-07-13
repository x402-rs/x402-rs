//! Strongly typed alloy bindings for the deployed `x402BatchSettlement`
//! contract and its EIP-712 typed-data structs.
//!
//! These bindings are derived from
//! `typescript/packages/mechanisms/evm/src/batch-settlement/abi.ts` and
//! cover every entry point the facilitator needs:
//! - `deposit`, `claim`, `claimWithSignature`, `settle`
//! - `refund`, `refundWithSignature`
//! - `initiateWithdraw`, `finalizeWithdraw`
//! - read-only views: `channels`, `pendingWithdrawals`, `refundNonce`,
//!   `getVoucherDigest`, `getRefundDigest`, `getClaimBatchDigest`, `getChannelId`,
//!   `CHANNEL_CONFIG_TYPEHASH`
//! - the `Settled` event
//! - the `multicall(bytes[])` aggregator (used for batched claim+refund)
//!
//! The EIP-712 structs (`ChannelConfig`, `Voucher`, `Refund`, `ClaimBatch`,
//! `ReceiveWithAuthorization`, `PermitWitnessTransferFrom`) live alongside the
//! contract ABI so they can be hashed through `alloy_sol_types::SolStruct`.

#![allow(missing_docs)]

use alloy_sol_types::sol;

sol! {
    /// Channel identity is derived from this immutable config struct.
    ///
    /// `channelId = EIP712Hash(ChannelConfig)` under the
    /// `x402 Batch Settlement` domain, binding the config to the EVM
    /// `chainId` and the deployed `x402BatchSettlement` address.
    #[derive(Debug)]
    struct ChannelConfig {
        address payer;
        address payerAuthorizer;
        address receiver;
        address receiverAuthorizer;
        address token;
        uint40 withdrawDelay;
        bytes32 salt;
    }

    /// Cumulative voucher: signed by the payer (or payer authorizer) to
    /// authorize the receiver to claim up to `maxClaimableAmount` from a
    /// specific channel.
    #[derive(Debug)]
    struct Voucher {
        bytes32 channelId;
        uint128 maxClaimableAmount;
    }

    /// Cooperative refund authorization: signed by the receiver authorizer
    /// to release a partial or full refund to the payer.
    #[derive(Debug)]
    struct Refund {
        bytes32 channelId;
        uint256 nonce;
        uint128 amount;
    }

    /// Batched claim entry — flattened form expected by `ClaimBatch`.
    #[derive(Debug)]
    struct ClaimEntry {
        bytes32 channelId;
        uint128 maxClaimableAmount;
        uint128 totalClaimed;
    }

    /// Batched claim authorization: signed by the receiver authorizer to
    /// authorize a `claimWithSignature` over several channels at once.
    #[derive(Debug)]
    struct ClaimBatch {
        ClaimEntry[] claims;
    }

    /// ERC-3009 `ReceiveWithAuthorization` typed data used by the
    /// `ERC3009DepositCollector` to pull funds from the payer into the
    /// batch-settlement escrow.
    #[derive(Debug)]
    struct ReceiveWithAuthorization {
        address from;
        address to;
        uint256 value;
        uint256 validAfter;
        uint256 validBefore;
        bytes32 nonce;
    }

    /// Permit2 token-permissions segment, used inside the channel-bound
    /// `PermitWitnessTransferFrom`.
    #[derive(Debug)]
    struct TokenPermissions {
        address token;
        uint256 amount;
    }

    /// Channel-bound witness for the Permit2 deposit collector.
    #[derive(Debug)]
    struct DepositWitness {
        bytes32 channelId;
    }

    /// Permit2 typed data for channel-bound batch deposits.
    #[derive(Debug)]
    struct PermitWitnessTransferFrom {
        TokenPermissions permitted;
        address spender;
        uint256 nonce;
        uint256 deadline;
        DepositWitness witness;
    }

    /// Voucher claim row consumed by `claim` / `claimWithSignature`.
    #[derive(Debug)]
    struct VoucherClaim {
        VoucherClaimInner voucher;
        bytes signature;
        uint128 totalClaimed;
    }

    /// Inner voucher view used by `VoucherClaim`. Mirrors the upstream ABI
    /// `voucher: { channel, maxClaimableAmount }` shape.
    #[derive(Debug)]
    struct VoucherClaimInner {
        ChannelConfig channel;
        uint128 maxClaimableAmount;
    }

    /// `x402BatchSettlement` contract bindings.
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    #[sol(rpc)]
    contract X402BatchSettlement {
        function multicall(bytes[] data) external returns (bytes[] results);

        function deposit(
            ChannelConfig config,
            uint128 amount,
            address collector,
            bytes collectorData
        ) external;

        function claim(VoucherClaim[] voucherClaims) external;

        function claimWithSignature(
            VoucherClaim[] voucherClaims,
            bytes authorizerSignature
        ) external;

        function settle(address receiver, address token) external;

        function initiateWithdraw(ChannelConfig config, uint128 amount) external;

        function finalizeWithdraw(ChannelConfig config) external;

        function refund(ChannelConfig config, uint128 amount) external;

        function refundWithSignature(
            ChannelConfig config,
            uint128 amount,
            uint256 nonce,
            bytes receiverAuthorizerSignature
        ) external;

        function getChannelId(ChannelConfig config) external view returns (bytes32);

        function CHANNEL_CONFIG_TYPEHASH() external view returns (bytes32);

        function channels(bytes32 channelId)
            external
            view
            returns (uint128 balance, uint128 totalClaimed);

        function pendingWithdrawals(bytes32 channelId)
            external
            view
            returns (uint128 amount, uint40 initiatedAt);

        function receivers(address receiver, address token)
            external
            view
            returns (uint128 totalClaimed, uint128 totalSettled);

        function getVoucherDigest(bytes32 channelId, uint128 maxClaimableAmount)
            external
            view
            returns (bytes32);

        function getRefundDigest(bytes32 channelId, uint256 nonce, uint128 amount)
            external
            view
            returns (bytes32);

        function refundNonce(bytes32 channelId) external view returns (uint256);

        function getClaimBatchDigest(VoucherClaim[] voucherClaims)
            external
            view
            returns (bytes32);

        event Settled(
            address indexed receiver,
            address indexed token,
            address indexed sender,
            uint128 amount
        );
    }

    /// Minimal ERC-20 interface for `balanceOf` / `allowance` reads.
    #[derive(Debug)]
    #[sol(rpc)]
    contract IERC20View {
        function balanceOf(address account) external view returns (uint256);
        function allowance(address owner, address spender) external view returns (uint256);
    }
}
