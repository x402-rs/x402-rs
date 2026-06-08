use alloy_primitives::U256;
use serde::{Deserialize, Serialize};
use x402_types::lit_str;
use x402_types::scheme::ExtensionKey;
use x402_types::timestamp::UnixTimestamp;

use crate::chain::{ChecksummedAddress, EOASignature};

/// Wrapper that contains the extension info nested under `info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip2612GasSponsoring {
    pub info: Eip2612GasSponsoringInfo,
}

impl ExtensionKey for Eip2612GasSponsoring {
    const EXTENSION_KEY: &'static str = "eip2612GasSponsoring";
}

/// EIP2612-gas-sponsoring extension provided by the server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip2612GasSponsoringServer {
    pub info: Eip2612GasSponsoringServerInfo,
    pub schema: Box<serde_json::value::RawValue>,
}

impl ExtensionKey for Eip2612GasSponsoringServer {
    const EXTENSION_KEY: &'static str = Eip2612GasSponsoring::EXTENSION_KEY;
}

lit_str!(Eip2612GasSponsoringV1, "1");

#[derive(Debug, Clone, Serialize, Deserialize)]
// FIXME DOc comments
pub struct Eip2612GasSponsoringServerInfo {
    pub description: String,
    pub version: Eip2612GasSponsoringV1,
}

/// Extension info provided by the client inside the `eip2612GasSponsoring` extension.
///
/// This is the EIP-2612 permit data the client has signed to approve the canonical
/// Permit2 contract to spend tokens on its behalf.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Eip2612GasSponsoringInfo {
    /// The address of the token owner (payer).
    pub from: ChecksummedAddress,
    /// ERC-20 token contract address.
    pub asset: ChecksummedAddress,
    /// The spender that was approved (MUST be the canonical Permit2 address).
    pub spender: ChecksummedAddress,
    /// The amount approved via `permit` (typically `MaxUint256`).
    #[serde(with = "crate::decimal_u256")]
    pub amount: U256,
    /// The EIP-2612 nonce (not used in `settleWithPermit` call but available for
    /// signature validation purposes).
    #[serde(with = "crate::decimal_u256")]
    pub nonce: U256,
    /// The deadline for the EIP-2612 permit signature.
    pub deadline: UnixTimestamp,
    /// The 65-byte concatenated EIP-2612 signature `r ++ s ++ v` as a hex bytes string.
    pub signature: EOASignature,
    /// Extension schema version (currently `"1"`).
    pub version: String,
}
