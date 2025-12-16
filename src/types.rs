//! Type definitions for the x402 protocol.
//!
//! This mirrors the structures and validation logic from official x402 SDKs (TypeScript/Go).
//! The key objects are `PaymentPayload`, `PaymentRequirements`, `VerifyResponse`, and `SettleResponse`,
//! which encode payment intent, authorization, and the result of verification/settlement.
//!
//! This module supports ERC-3009 style authorization for tokens (EIP-712 typed signatures),
//! and provides serialization logic compatible with external clients.

use alloy_primitives::hex;
use alloy_primitives::{Bytes, U256};
use alloy_sol_types::sol;
use base64::Engine;
use base64::engine::general_purpose::STANDARD as b64;
use once_cell::sync::Lazy;
use regex::Regex;
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, Zero};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::ops::{Add, Div, Mul, Rem, Sub};
use std::str::FromStr;
use url::Url;
use crate::b64::Base64Bytes;
use crate::network::Network;
use crate::p1::chain::ChainId;
use crate::p1::chain::solana;
use crate::proto;
use crate::timestamp::UnixTimestamp;

pub use crate::proto::scheme::Scheme;

/// Represents an EVM signature used in EIP-712 typed data.
/// Serialized as 0x-prefixed hex string.
/// Used to authorize an ERC-3009 transferWithAuthorization.
/// Can contain EOA, EIP-1271, and EIP-6492 signatures.
#[derive(Clone, PartialEq, Eq)]
pub struct EvmSignature(pub Vec<u8>);

impl From<[u8; 65]> for EvmSignature {
    fn from(bytes: [u8; 65]) -> Self {
        EvmSignature(bytes.to_vec())
    }
}

impl From<Bytes> for EvmSignature {
    fn from(bytes: Bytes) -> Self {
        EvmSignature(bytes.to_vec())
    }
}

impl From<EvmSignature> for Bytes {
    fn from(value: EvmSignature) -> Self {
        Bytes::from(value.0)
    }
}

impl Debug for EvmSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EvmSignature(0x{})", hex::encode(self.0.clone()))
    }
}

impl<'de> Deserialize<'de> for EvmSignature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(s.trim_start_matches("0x"))
            .map_err(|_| serde::de::Error::custom("Failed to decode EVM signature hex string"))?;

        Ok(EvmSignature(bytes))
    }
}

impl Serialize for EvmSignature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex_string = format!("0x{}", hex::encode(self.0.clone()));
        serializer.serialize_str(&hex_string)
    }
}

/// Represents a 32-byte random nonce, hex-encoded with 0x prefix.
/// Must be exactly 64 hex characters long.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct HexEncodedNonce(pub [u8; 32]);

impl Debug for HexEncodedNonce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "HexEncodedNonce(0x{})", hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for HexEncodedNonce {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        static NONCE_REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^0x[0-9a-fA-F]{64}$").expect("Invalid nonce regex"));

        if !NONCE_REGEX.is_match(&s) {
            return Err(serde::de::Error::custom("Invalid nonce format"));
        }

        let bytes =
            hex::decode(&s[2..]).map_err(|_| serde::de::Error::custom("Invalid hex in nonce"))?;

        let array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("Invalid length for nonce"))?;

        Ok(HexEncodedNonce(array))
    }
}

impl Serialize for HexEncodedNonce {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex_string = format!("0x{}", hex::encode(self.0));
        serializer.serialize_str(&hex_string)
    }
}

/// EIP-712 structured data for ERC-3009-based authorization.
/// Defines who can transfer how much USDC and when.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactEvmPayloadAuthorization {
    pub from: alloy_primitives::Address,
    pub to: alloy_primitives::Address,
    pub value: TokenAmount,
    pub valid_after: UnixTimestamp,
    pub valid_before: UnixTimestamp,
    pub nonce: HexEncodedNonce,
}

/// Full payload required to authorize an ERC-3009 transfer:
/// includes the signature and the EIP-712 struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactEvmPayload {
    pub signature: EvmSignature,
    pub authorization: ExactEvmPayloadAuthorization,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactSolanaPayload {
    pub transaction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ExactPaymentPayload {
    Evm(ExactEvmPayload),
    Solana(ExactSolanaPayload),
}

/// Describes a signed request to transfer a specific amount of funds on-chain.
/// Includes the scheme, network, and signed payload contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload {
    pub x402_version: proto::X402Version,
    pub scheme: Scheme,
    pub network: Network,
    pub payload: ExactPaymentPayload,
}

/// Error returned when decoding a base64-encoded [`PaymentPayload`] fails.
///
/// This error type is used by a payment-gated endpoint or a facilitator to signal that the client-supplied
/// `X-Payment` header could not be decoded into a valid [`PaymentPayload`].
#[derive(Debug, thiserror::Error)]
pub enum PaymentPayloadB64DecodingError {
    /// The input bytes were not valid base64.
    #[error("base64 decode error: {0}")]
    Base64Decode(#[from] base64::DecodeError),

    /// The decoded bytes could not be interpreted as a UTF-8 JSON string.
    #[error("utf-8 decode error: {0}")]
    Utf8(#[from] std::str::Utf8Error),

    /// The JSON structure was invalid or did not conform to [`PaymentPayload`].
    #[error("json parse error: {0}")]
    Json(#[from] serde_json::Error),
}

/// A precise on-chain token amount in base units (e.g., USDC with 6 decimals).
/// Represented as a stringified `U256` in JSON to prevent precision loss.
#[derive(Debug, Copy, Clone, PartialEq, Ord, PartialOrd, Eq, Hash)]
pub struct TokenAmount(pub U256);

impl TokenAmount {
    /// Computes the absolute difference between `self` and `other`.
    ///
    /// Returns $\left\vert \mathtt{self} - \mathtt{other} \right\vert$.
    #[must_use]
    pub fn abs_diff(self, other: Self) -> Self {
        Self(self.0.abs_diff(other.0))
    }

    /// Computes `self + rhs`, returning [`None`] if overflow occurred.
    #[must_use]
    pub const fn checked_add(self, rhs: Self) -> Option<Self> {
        match self.0.checked_add(rhs.0) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Computes `-self`, returning [`None`] unless `self == 0`.
    #[must_use]
    pub const fn checked_neg(self) -> Option<Self> {
        match self.0.checked_neg() {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Computes `self - rhs`, returning [`None`] if overflow occurred.
    #[must_use]
    pub const fn checked_sub(self, rhs: Self) -> Option<Self> {
        match self.0.checked_sub(rhs.0) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    /// Calculates $\mod{\mathtt{self} + \mathtt{rhs}}_{2^{BITS}}$.
    ///
    /// Returns a tuple of the addition along with a boolean indicating whether
    /// an arithmetic overflow would occur. If an overflow would have occurred
    /// then the wrapped value is returned.
    #[must_use]
    pub const fn overflowing_add(self, rhs: Self) -> (Self, bool) {
        let add = self.0.overflowing_add(rhs.0);
        (Self(add.0), add.1)
    }

    /// Calculates $\mod{-\mathtt{self}}_{2^{BITS}}$.
    ///
    /// Returns `!self + 1` using wrapping operations to return the value that
    /// represents the negation of this unsigned value. Note that for positive
    /// unsigned values overflow always occurs, but negating 0 does not
    /// overflow.
    #[must_use]
    pub const fn overflowing_neg(self) -> (Self, bool) {
        let neg = self.0.overflowing_neg();
        (Self(neg.0), neg.1)
    }

    /// Calculates $\mod{\mathtt{self} - \mathtt{rhs}}_{2^{BITS}}$.
    ///
    /// Returns a tuple of the subtraction along with a boolean indicating
    /// whether an arithmetic overflow would occur. If an overflow would have
    /// occurred then the wrapped value is returned.
    #[must_use]
    pub const fn overflowing_sub(self, rhs: Self) -> (Self, bool) {
        let sub = self.0.overflowing_sub(rhs.0);
        (Self(sub.0), sub.1)
    }

    /// Computes `self + rhs`, saturating at the numeric bounds instead of
    /// overflowing.
    #[must_use]
    pub const fn saturating_add(self, rhs: Self) -> Self {
        Self(self.0.saturating_add(rhs.0))
    }

    /// Computes `self - rhs`, saturating at the numeric bounds instead of
    /// overflowing
    #[must_use]
    pub const fn saturating_sub(self, rhs: Self) -> Self {
        Self(self.0.saturating_sub(rhs.0))
    }

    /// Computes `self + rhs`, wrapping around at the boundary of the type.
    #[must_use]
    pub const fn wrapping_add(self, rhs: Self) -> Self {
        Self(self.0.wrapping_add(rhs.0))
    }

    /// Computes `-self`, wrapping around at the boundary of the type.
    #[must_use]
    pub const fn wrapping_neg(self) -> Self {
        self.overflowing_neg().0
    }

    /// Computes `self - rhs`, wrapping around at the boundary of the type.
    #[must_use]
    pub const fn wrapping_sub(self, rhs: Self) -> Self {
        self.overflowing_sub(rhs).0
    }

    /// Computes `self * rhs`, returning [`None`] if overflow occurred.
    #[inline(always)]
    #[must_use]
    pub fn checked_mul(self, rhs: Self) -> Option<Self> {
        match self.overflowing_mul(rhs) {
            (value, false) => Some(value),
            _ => None,
        }
    }

    /// Calculates the multiplication of self and rhs.
    ///
    /// Returns a tuple of the multiplication along with a boolean indicating
    /// whether an arithmetic overflow would occur. If an overflow would have
    /// occurred then the wrapped value is returned.
    #[inline]
    #[must_use]
    pub fn overflowing_mul(self, rhs: Self) -> (Self, bool) {
        let (mul, overflow) = self.0.overflowing_mul(rhs.0);
        (Self(mul), overflow)
    }

    /// Computes `self * rhs`, saturating at the numeric bounds instead of
    /// overflowing.
    #[inline(always)]
    #[must_use]
    pub fn saturating_mul(self, rhs: Self) -> Self {
        Self(self.0.saturating_mul(rhs.0))
    }

    /// Computes `self * rhs`, wrapping around at the boundary of the type.
    #[inline(always)]
    #[must_use]
    pub fn wrapping_mul(self, rhs: Self) -> Self {
        Self(self.0.wrapping_mul(rhs.0))
    }

    /// Computes the inverse modulo $2^{\mathtt{BITS}}$ of `self`, returning
    /// [`None`] if the inverse does not exist.
    #[inline]
    #[must_use]
    pub fn inv_ring(self) -> Option<Self> {
        self.0.inv_ring().map(Self)
    }

    /// Computes `self / rhs`, returning [`None`] if `rhs == 0`.
    #[inline]
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // False positive
    pub fn checked_div(self, rhs: Self) -> Option<Self> {
        self.0.checked_div(rhs.0).map(Self)
    }

    /// Computes `self % rhs`, returning [`None`] if `rhs == 0`.
    #[inline]
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // False positive
    pub fn checked_rem(self, rhs: Self) -> Option<Self> {
        self.0.checked_rem(rhs.0).map(Self)
    }

    /// Computes `self / rhs` rounding up.
    ///
    /// # Panics
    ///
    /// Panics if `rhs == 0`.
    #[inline]
    #[must_use]
    #[track_caller]
    pub fn div_ceil(self, rhs: Self) -> Self {
        Self(self.0.div_ceil(rhs.0))
    }

    /// Computes `self / rhs` and `self % rhs`.
    ///
    /// # Panics
    ///
    /// Panics if `rhs == 0`.
    #[inline]
    #[must_use]
    #[track_caller]
    pub fn div_rem(self, rhs: Self) -> (Self, Self) {
        let (d, m) = self.0.div_rem(rhs.0);
        (Self(d), Self(m))
    }

    /// Computes `self / rhs` rounding down.
    ///
    /// # Panics
    ///
    /// Panics if `rhs == 0`.
    #[inline]
    #[must_use]
    #[track_caller]
    pub fn wrapping_div(self, rhs: Self) -> Self {
        self.div_rem(rhs).0
    }

    /// Computes `self % rhs`.
    ///
    /// # Panics
    ///
    /// Panics if `rhs == 0`.
    #[inline]
    #[must_use]
    #[track_caller]
    pub fn wrapping_rem(self, rhs: Self) -> Self {
        self.div_rem(rhs).1
    }
}

impl From<TokenAmount> for U256 {
    fn from(value: TokenAmount) -> Self {
        value.0
    }
}

impl From<U256> for TokenAmount {
    fn from(value: U256) -> Self {
        TokenAmount(value)
    }
}

impl<T: Into<TokenAmount>> Add<T> for TokenAmount {
    type Output = TokenAmount;

    fn add(self, rhs: T) -> Self::Output {
        self.wrapping_add(rhs.into())
    }
}

impl<T: Into<TokenAmount>> Sub<T> for TokenAmount {
    type Output = TokenAmount;
    fn sub(self, rhs: T) -> Self::Output {
        self.wrapping_sub(rhs.into())
    }
}

impl<T: Into<TokenAmount>> Mul<T> for TokenAmount {
    type Output = TokenAmount;
    fn mul(self, rhs: T) -> Self::Output {
        self.wrapping_mul(rhs.into())
    }
}

impl<T: Into<TokenAmount>> Div<T> for TokenAmount {
    type Output = TokenAmount;
    fn div(self, rhs: T) -> Self::Output {
        self.wrapping_div(rhs.into())
    }
}

impl<T: Into<TokenAmount>> Rem<T> for TokenAmount {
    type Output = TokenAmount;
    fn rem(self, rhs: T) -> Self::Output {
        self.wrapping_rem(rhs.into())
    }
}

impl Zero for TokenAmount {
    fn zero() -> Self {
        TokenAmount(U256::from(0))
    }

    fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
}

impl<'de> Deserialize<'de> for TokenAmount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let string = String::deserialize(deserializer)?;
        let value = U256::from_str(&string).map_err(serde::de::Error::custom)?;
        Ok(TokenAmount(value))
    }
}

impl Serialize for TokenAmount {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl Display for TokenAmount {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u128> for TokenAmount {
    fn from(value: u128) -> Self {
        TokenAmount(U256::from(value))
    }
}

impl From<u64> for TokenAmount {
    fn from(value: u64) -> Self {
        TokenAmount(U256::from(value))
    }
}

/// Represents either an EVM address (0x...), or an off-chain address, or Solana address.
/// The format is used for routing settlement.
#[derive(Debug, Hash, Clone, PartialEq, Eq)]
pub enum MixedAddress {
    /// EVM address
    Evm(alloy_primitives::Address),
    /// Solana address
    Solana(solana::Address),
    /// Off-chain address in `^[A-Za-z0-9][A-Za-z0-9-]{0,34}[A-Za-z0-9]$` format.
    Offchain(String),
}

#[macro_export]
macro_rules! address_evm {
    ($s:literal) => {
        $crate::types::MixedAddress::Evm($crate::__reexports::alloy_primitives::address!($s).into())
    };
}

#[macro_export]
macro_rules! address_sol {
    ($s:literal) => {
        $crate::types::MixedAddress::Solana($crate::chain::solana::Address::new(
            $crate::__reexports::solana_pubkey::pubkey!($s),
        ))
    };
}

impl From<alloy_primitives::Address> for MixedAddress {
    fn from(value: alloy_primitives::Address) -> Self {
        MixedAddress::Evm(value.into())
    }
}

impl TryFrom<MixedAddress> for alloy_primitives::Address {
    type Error = MixedAddressError;

    fn try_from(value: MixedAddress) -> Result<Self, Self::Error> {
        match value {
            MixedAddress::Evm(address) => Ok(address.into()),
            MixedAddress::Offchain(_) => Err(MixedAddressError::NotEvmAddress),
            MixedAddress::Solana(_) => Err(MixedAddressError::NotEvmAddress),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MixedAddressError {
    #[error("Not an EVM address")]
    NotEvmAddress,
    #[error("Invalid address format")]
    InvalidAddressFormat,
}

impl Display for MixedAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MixedAddress::Evm(address) => write!(f, "{address}"),
            MixedAddress::Offchain(address) => write!(f, "{address}"),
            MixedAddress::Solana(pubkey) => write!(f, "{pubkey}"),
        }
    }
}

impl<'de> Deserialize<'de> for MixedAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        static OFFCHAIN_ADDRESS_REGEX: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"^[A-Za-z0-9][A-Za-z0-9-]{0,34}[A-Za-z0-9]$")
                .expect("Invalid regex for offchain address")
        });

        let s = String::deserialize(deserializer)?;
        // 1) EVM address (e.g., 0x... 20 bytes, hex)
        if let Ok(addr) = alloy_primitives::Address::from_str(&s) {
            return Ok(MixedAddress::Evm(addr));
        }
        // 2) Solana Pubkey (base58, 32 bytes)
        if let Ok(pk) = solana::Address::from_str(&s) {
            return Ok(MixedAddress::Solana(pk));
        }
        // 3) Off-chain address by regex
        if OFFCHAIN_ADDRESS_REGEX.is_match(&s) {
            return Ok(MixedAddress::Offchain(s));
        }
        Err(serde::de::Error::custom("Invalid address format"))
    }
}

impl Serialize for MixedAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            MixedAddress::Evm(addr) => serializer.serialize_str(&addr.to_string()),
            MixedAddress::Offchain(s) => serializer.serialize_str(s),
            MixedAddress::Solana(pubkey) => serializer.serialize_str(pubkey.to_string().as_str()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransactionHash {
    /// A 32-byte EVM transaction hash, encoded as 0x-prefixed hex string.
    Evm([u8; 32]),
    Solana([u8; 64]),
}

impl<'de> Deserialize<'de> for TransactionHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;

        static EVM_TX_HASH_REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^0x[0-9a-fA-F]{64}$").expect("invalid regex"));

        // EVM: 0x-prefixed, 32 bytes hex
        if EVM_TX_HASH_REGEX.is_match(&s) {
            let bytes = hex::decode(s.trim_start_matches("0x"))
                .map_err(|_| serde::de::Error::custom("Invalid hex in transaction hash"))?;
            let array: [u8; 32] = bytes.try_into().map_err(|_| {
                serde::de::Error::custom("Transaction hash must be exactly 32 bytes")
            })?;
            return Ok(TransactionHash::Evm(array));
        }

        // Solana: base58 string, decodes to exactly 64 bytes
        if let Ok(bytes) = bs58::decode(&s).into_vec()
            && bytes.len() == 64
        {
            let array: [u8; 64] = bytes.try_into().unwrap(); // safe after length check
            return Ok(TransactionHash::Solana(array));
        }

        Err(serde::de::Error::custom("Invalid transaction hash format"))
    }
}

impl Serialize for TransactionHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            TransactionHash::Evm(bytes) => {
                let hex_string = format!("0x{}", hex::encode(bytes));
                serializer.serialize_str(&hex_string)
            }
            TransactionHash::Solana(bytes) => {
                let b58_string = bs58::encode(bytes).into_string();
                serializer.serialize_str(&b58_string)
            }
        }
    }
}

impl Display for TransactionHash {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            TransactionHash::Evm(bytes) => {
                write!(f, "0x{}", hex::encode(bytes))
            }
            TransactionHash::Solana(bytes) => {
                write!(f, "{}", bs58::encode(bytes).into_string())
            }
        }
    }
}

/// Requirements set by the payment-gated endpoint for an acceptable payment.
/// This includes min/max amounts, recipient, asset, network, and metadata.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements {
    pub scheme: Scheme,
    pub network: Network,
    pub max_amount_required: TokenAmount,
    pub resource: Url,
    pub description: String,
    pub mime_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
    pub pay_to: MixedAddress,
    pub max_timeout_seconds: u64,
    pub asset: MixedAddress,
    pub extra: Option<serde_json::Value>,
}

/// Wrapper for a payment payload and requirements sent by the client to a facilitator
/// to be verified.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest {
    pub x402_version: proto::X402Version,
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
}

impl Display for VerifyRequest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "VerifyRequest(version={:?}, payment_payload={:?}, payment_requirements={:?})",
            self.x402_version, self.payment_payload, self.payment_requirements
        )
    }
}

impl VerifyRequest {
    pub fn network(&self) -> Network {
        self.payment_payload.network
    }
}

/// Wrapper for a payment payload and requirements sent by the client
/// to be used for settlement.
pub type SettleRequest = VerifyRequest;

#[derive(Debug, Serialize, Deserialize, thiserror::Error)]
#[serde(untagged, rename_all = "camelCase")]
pub enum FacilitatorErrorReason {
    /// Payer doesn't have sufficient funds.
    #[error("insufficient_funds")]
    #[serde(rename = "insufficient_funds")]
    InsufficientFunds,
    /// The scheme in PaymentPayload didn't match expected (e.g., not 'exact'), or settlement failed.
    #[error("invalid_scheme")]
    #[serde(rename = "invalid_scheme")]
    InvalidScheme,
    /// Network in PaymentPayload didn't match a facilitator's expected network.
    #[error("invalid_network")]
    #[serde(rename = "invalid_network")]
    InvalidNetwork,
    /// Unexpected settle error
    #[error("unexpected_settle_error")]
    #[serde(rename = "unexpected_settle_error")]
    UnexpectedSettleError,
    #[error("{0}")]
    FreeForm(String),
}

/// Returned from a facilitator after attempting to settle a payment on-chain.
/// Indicates success/failure, transaction hash, and payer identity.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<FacilitatorErrorReason>,
    pub payer: MixedAddress,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<TransactionHash>,
    pub network: Network,
}
