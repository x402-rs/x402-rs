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
    address!("0x4020615294c913F045dc10f0a5cdEbd86c280001");

// TODO configurable address per chain
/// The X402 UptoPermit2Proxy contract address for settling Permit2 payments with variable amounts.
/// This contract allows settling for any amount up to the permitted maximum.
pub const UPTO_PERMIT2_PROXY_ADDRESS: Address =
    address!("0x4020633461b2895a48930ff97ee8fcde8e520002");

/// Authorization details for a Permit2 call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Authorization {
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
    pub witness: Permit2Witness,
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
/// Binds the recipient address to prevent the facilitator from redirecting funds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Witness {
    /// Extra data (can be empty for basic transfers).
    pub extra: Bytes,
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
pub struct Permit2Payload {
    pub permit_2_authorization: Permit2Authorization,
    pub signature: Bytes,
}
