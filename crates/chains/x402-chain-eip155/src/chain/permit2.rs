use alloy_primitives::{Address, Bytes, U256, address};
use serde::{Deserialize, Serialize};
use x402_types::timestamp::UnixTimestamp;

use crate::chain::ChecksummedAddress;

// TODO configurable address per chain
/// The canonical Permit2 contract address deployed on most chains.
pub const PERMIT2_ADDRESS: Address = address!("0x000000000022D473030F116dDEE9F6B43aC78BA3");

// TODO configurable address per chain
/// The X402 ExactPermit2Proxy contract address for settling Permit2 payments.
pub const EXACT_PERMIT2_PROXY_ADDRESS: Address =
    address!("0x402085c248EeA27D92E8b30b2C58ed07f9E20001");

// TODO configurable address per chain
/// The X402 UptoPermit2Proxy contract address for settling Permit2 payments with variable amounts.
/// This contract allows settling for any amount up to the permitted maximum.
///
/// Canonical address per x402-foundation/x402 commit ad2658a (PR #1880), matches the
/// `@x402/evm` SDK constant `x402UptoPermit2ProxyAddress`. Deployed at this CREATE2
/// address on Base mainnet + Base Sepolia + other EVM mainnets that ship UPTO.
pub const UPTO_PERMIT2_PROXY_ADDRESS: Address =
    address!("0x4020A4f3b7b90ccA423B9fabCc0CE57C6C240002");

/// Authorization details for a Permit2 call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Authorization<TWitness> {
    /// Deadline after which the authorization expires.
    pub deadline: UnixTimestamp,
    /// The address authorizing the transfer (the payer).
    pub from: ChecksummedAddress,
    /// Unique nonce for replay protection.
    #[serde(with = "crate::decimal_u256")]
    pub nonce: U256,
    /// The token and maximum amount permitted.
    pub permitted: Permit2AuthorizationPermitted,
    /// The spender address (must be the X402 Permit2Proxy).
    pub spender: ChecksummedAddress,
    /// Witness data binding the recipient.
    pub witness: TWitness,
}

/// Token and amount details for Permit2 authorization.
///
/// The `amount` is the maximum that can be charged at settlement.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2AuthorizationPermitted {
    /// Maximum amount that can be transferred.
    #[serde(with = "crate::decimal_u256")]
    pub amount: U256,
    /// Token contract address.
    pub token: ChecksummedAddress,
}

/// Witness data for Permit2 upto payments.
///
/// Binds the recipient address AND the authorized facilitator EOA, so only the
/// caller whose address matches `facilitator` can invoke `settle` on the proxy.
/// Matches `Witness(address to, address facilitator, uint256 validAfter)` on
/// `x402UptoPermit2Proxy` per x402-foundation commit ad2658a.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UptoPermit2Witness {
    /// The recipient address that will receive the funds.
    pub to: ChecksummedAddress,
    /// The facilitator EOA authorized to invoke settle (`msg.sender` at settle time).
    pub facilitator: ChecksummedAddress,
    /// Time after which the authorization becomes valid.
    pub valid_after: UnixTimestamp,
}

/// Witness data for Permit2 exact payments.
///
/// Binds the recipient address to prevent the facilitator from redirecting funds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactPermit2Witness {
    /// The recipient address that will receive the funds.
    pub to: ChecksummedAddress,
    /// Time after which the authorization becomes valid.
    pub valid_after: UnixTimestamp,
}

/// Payload for Permit2-based payments.
///
/// Contains the authorization details and signature for a Permit2 transfer
/// where the actual settled amount may be less than the authorized maximum,
/// depending on the scheme/proxy contract used.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Payload<TWitness> {
    pub permit_2_authorization: Permit2Authorization<TWitness>,
    pub signature: Bytes,
}

pub type ExactPermit2Payload = Permit2Payload<ExactPermit2Witness>;
pub type UptoPermit2Payload = Permit2Payload<UptoPermit2Witness>;
