//! EIP-2612 gas-sponsoring extension types for the x402 protocol.
//!
//! The `eip2612GasSponsoring` extension allows a facilitator to accept an
//! off-chain EIP-2612 `permit` signature from the buyer and submit it to the
//! canonical Permit2 contract on-chain, paying the gas fees on the buyer's behalf.
//!
//! ## Wire format
//!
//! A facilitator advertises support by including the `eip2612GasSponsoring` key in
//! the `extensions` object of the `402 Payment Required` response
//! ([`Eip2612GasSponsoringServer`]).  The buyer then populates that same key in the
//! `extensions` field of the payment payload ([`Eip2612GasSponsoring`]) with the
//! signed permit data.

use alloy_primitives::U256;
use serde::{Deserialize, Serialize};
use x402_types::lit_str;
use x402_types::scheme::ExtensionKey;
use x402_types::timestamp::UnixTimestamp;

#[cfg(feature = "client")]
use alloy_sol_types::sol;

use crate::chain::{ChecksummedAddress, EOASignature};

/// Client-side `eip2612GasSponsoring` extension payload sent inside a payment.
///
/// The buyer includes this in the `extensions` map of a
/// [`PaymentPayload`](x402_types::proto::v2::PaymentPayload) to supply the
/// EIP-2612 permit data that the facilitator will submit on-chain.
///
/// The [`ExtensionKey::EXTENSION_KEY`] for this type is `"eip2612GasSponsoring"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip2612GasSponsoring {
    /// The signed EIP-2612 permit data provided by the client.
    pub info: Eip2612GasSponsoringInfo,
}

impl ExtensionKey for Eip2612GasSponsoring {
    const EXTENSION_KEY: &'static str = "eip2612GasSponsoring";
}

/// Server-side `eip2612GasSponsoring` extension advertisement sent in a 402 response.
///
/// A facilitator includes this in the `extensions` map of a
/// [`PaymentRequired`](x402_types::proto::v2::PaymentRequired) response to
/// signal support for the EIP-2612 gasless approval flow.  The `schema` field
/// carries a JSON Schema object that describes the shape of the client payload
/// the facilitator expects to receive.
///
/// The [`ExtensionKey::EXTENSION_KEY`] for this type is `"eip2612GasSponsoring"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip2612GasSponsoringServer {
    /// Human-readable extension metadata advertised by the facilitator.
    pub info: Eip2612GasSponsoringServerInfo,
    /// JSON Schema describing the expected client payload structure.
    pub schema: Box<serde_json::value::RawValue>,
}

impl ExtensionKey for Eip2612GasSponsoringServer {
    const EXTENSION_KEY: &'static str = Eip2612GasSponsoring::EXTENSION_KEY;
}

lit_str!(Eip2612GasSponsoringV1, "1");

/// Metadata the facilitator advertises for the `eip2612GasSponsoring` extension.
///
/// Included inside [`Eip2612GasSponsoringServer`] under the `info` key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip2612GasSponsoringServerInfo {
    /// Human-readable description of what the facilitator does with the permit.
    pub description: String,
    /// Extension schema version; currently always `"1"`.
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
    /// Extension schema version.
    pub version: Eip2612GasSponsoringV1,
}

#[cfg(feature = "client")]
sol! {
    #[allow(missing_docs)]
    #[derive(Debug)]
    /// ABI-encoded EIP-2612 `Permit` struct used to construct the typed-data hash
    /// for signature verification.
    ///
    /// This mirrors the on-chain `Permit` struct defined in the EIP-2612 standard
    /// and is used when encoding the EIP-712 `PERMIT_TYPEHASH` payload.
    struct Permit {
        address owner;
        address spender;
        uint256 value;
        uint256 nonce;
        uint256 deadline;
    }
}
