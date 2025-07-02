//! Type definitions for the x402 protocol.
//!
//! This mirrors the structures and validation logic from official x402 SDKs (TypeScript/Go).
//! The key objects are `PaymentPayload`, `PaymentRequirements`, `VerifyResponse`, and `SettleResponse`,
//! which encode payment intent, authorization, and the result of verification/settlement.
//!
//! This module supports ERC-3009 style authorization for tokens (EIP-712 typed signatures),
//! and provides serialization logic compatible with external clients.

use alloy::primitives::U256;
use alloy::{hex, sol};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as b64;
use once_cell::sync::Lazy;
use regex::Regex;
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use serde::ser::SerializeStruct;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::borrow::Cow;
use std::fmt;
use std::fmt::{Debug, Display};
use std::ops::Mul;
use std::str::FromStr;
use url::Url;

use crate::network::Network;

/// Represents the protocol version. Currently only version 1 is supported.
#[derive(Debug, Copy, Clone)]
pub enum X402Version {
    /// Version `1`.
    V1,
}

impl Serialize for X402Version {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            X402Version::V1 => serializer.serialize_u8(1),
        }
    }
}

impl Display for X402Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            X402Version::V1 => write!(f, "1"),
        }
    }
}

#[derive(Debug)]
pub struct X402VersionError(pub u8);

impl Display for X402VersionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unsupported x402Version: {}", self.0)
    }
}

impl std::error::Error for X402VersionError {}

impl TryFrom<u8> for X402Version {
    type Error = X402VersionError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(X402Version::V1),
            _ => Err(X402VersionError(value)),
        }
    }
}

impl<'de> Deserialize<'de> for X402Version {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let num = u8::deserialize(deserializer)?;
        X402Version::try_from(num).map_err(serde::de::Error::custom)
    }
}

/// Enumerates payment schemes. Only "exact" is supported in this implementation,
/// meaning the amount to be transferred must match exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scheme {
    Exact,
}

impl Display for Scheme {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            Scheme::Exact => "exact",
        };
        write!(f, "{}", s)
    }
}

/// Represents a 65-byte EVM signature used in EIP-712 typed data.
/// Serialized as 0x-prefixed hex string with 130 characters.
/// Used to authorize an ERC-3009 transferWithAuthorization.
#[derive(Copy, Clone, PartialEq, Eq)]
pub struct EvmSignature(pub [u8; 65]);

impl From<[u8; 65]> for EvmSignature {
    fn from(bytes: [u8; 65]) -> Self {
        EvmSignature(bytes)
    }
}

impl Debug for EvmSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "EvmSignature(0x{})", hex::encode(self.0))
    }
}

impl<'de> Deserialize<'de> for EvmSignature {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        static SIG_REGEX: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"^0x[0-9a-fA-F]{130}$").expect("Invalid regex for EVM signature")
        });

        if SIG_REGEX.is_match(&s) {
            let bytes = hex::decode(s.trim_start_matches("0x")).map_err(|_| {
                serde::de::Error::custom("Failed to decode EVM signature hex string")
            })?;

            let array: [u8; 65] = bytes
                .try_into()
                .map_err(|_| serde::de::Error::custom("Signature must be exactly 65 bytes"))?;

            Ok(EvmSignature(array))
        } else {
            Err(serde::de::Error::custom(
                "Invalid EVM signature format: must be 0x-prefixed and 130 hex chars",
            ))
        }
    }
}

impl Serialize for EvmSignature {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex_string = format!("0x{}", hex::encode(self.0));
        serializer.serialize_str(&hex_string)
    }
}

/// Represents an EVM address.
///
/// Wrapper around `alloy::primitives::Address`, providing display/serialization support.
/// Used throughout the protocol for typed Ethereum address handling.
#[derive(Debug, Copy, Clone, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct EvmAddress(pub alloy::primitives::Address);

impl Display for EvmAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, thiserror::Error)]
#[error("Failed to decode EVM address")]
pub struct EvmAddressDecodingError;

impl FromStr for EvmAddress {
    type Err = EvmAddressDecodingError;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let address =
            alloy::primitives::Address::from_str(s).map_err(|_| EvmAddressDecodingError)?;
        Ok(Self(address))
    }
}

impl TryFrom<&str> for EvmAddress {
    type Error = EvmAddressDecodingError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::from_str(value)
    }
}

impl From<EvmAddress> for alloy::primitives::Address {
    fn from(address: EvmAddress) -> Self {
        address.0
    }
}

impl From<alloy::primitives::Address> for EvmAddress {
    fn from(address: alloy::primitives::Address) -> Self {
        EvmAddress(address)
    }
}

impl PartialEq<alloy::primitives::Address> for EvmAddress {
    fn eq(&self, other: &alloy::primitives::Address) -> bool {
        let other = *other;
        self.0 == other
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

/// A Unix timestamp represented as a `u64`, used in payment authorization windows.
///
/// This type encodes the number of seconds since the Unix epoch (1970-01-01T00:00:00Z).
/// It is used in time-bounded ERC-3009 `transferWithAuthorization` messages to specify
/// the validity window (`validAfter` and `validBefore`) of a payment authorization.
///
/// Serialized as a stringified integer to avoid loss of precision in JSON.
/// For example, `1699999999` becomes `"1699999999"` in the wire format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UnixTimestamp(pub u64);

impl Serialize for UnixTimestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for UnixTimestamp {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let ts = s
            .parse::<u64>()
            .map_err(|_| serde::de::Error::custom("timestamp must be a non-negative integer"))?;
        Ok(UnixTimestamp(ts))
    }
}

impl From<UnixTimestamp> for U256 {
    fn from(value: UnixTimestamp) -> Self {
        U256::from(value.0)
    }
}

/// EIP-712 structured data for ERC-3009-based authorization.
/// Defines who can transfer how much USDC and when.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactEvmPayloadAuthorization {
    pub from: EvmAddress,
    pub to: EvmAddress,
    pub value: TokenAmount,
    pub valid_after: UnixTimestamp,
    pub valid_before: UnixTimestamp,
    pub nonce: HexEncodedNonce,
}

/// Full payload required to authorize an ERC-3009 transfer:
/// includes the signature and the EIP-712 struct.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExactEvmPayload {
    pub signature: EvmSignature,
    pub authorization: ExactEvmPayloadAuthorization,
}

/// Describes a signed request to transfer a specific amount of funds on-chain.
/// Includes the scheme, network, and signed payload contents.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload {
    pub x402_version: X402Version,
    pub scheme: Scheme,
    pub network: Network,
    pub payload: ExactEvmPayload,
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

impl TryFrom<Base64Bytes<'_>> for PaymentPayload {
    type Error = PaymentPayloadB64DecodingError;

    fn try_from(value: Base64Bytes) -> Result<Self, Self::Error> {
        let decoded = value.decode()?;
        serde_json::from_slice(&decoded).map_err(PaymentPayloadB64DecodingError::from)
    }
}

/// A precise on-chain token amount in base units (e.g., USDC with 6 decimals).
/// Represented as a stringified `U256` in JSON to prevent precision loss.
pub type TokenAmount = U256;

/// Represents either an EVM address (0x...) or an off-chain address.
/// The format is validated by regex and used for routing settlement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MixedAddress {
    /// EVM address
    Evm(EvmAddress),
    /// Off-chain address in `^[A-Za-z0-9][A-Za-z0-9-]{0,34}[A-Za-z0-9]$` format.
    Offchain(String),
}

impl From<alloy::primitives::Address> for MixedAddress {
    fn from(value: alloy::primitives::Address) -> Self {
        MixedAddress::Evm(value.into())
    }
}

impl TryFrom<MixedAddress> for alloy::primitives::Address {
    type Error = MixedAddressError;

    fn try_from(value: MixedAddress) -> Result<Self, Self::Error> {
        match value {
            MixedAddress::Evm(address) => Ok(address.into()),
            MixedAddress::Offchain(_) => Err(MixedAddressError::NotEvmAddress),
        }
    }
}

impl From<EvmAddress> for MixedAddress {
    fn from(address: EvmAddress) -> Self {
        MixedAddress::Evm(address)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MixedAddressError {
    #[error("Not an EVM address")]
    NotEvmAddress,
    #[error("Invalid address format")]
    InvalidAddressFormat,
}

impl TryInto<EvmAddress> for MixedAddress {
    type Error = MixedAddressError;

    fn try_into(self) -> Result<EvmAddress, Self::Error> {
        match self {
            MixedAddress::Evm(address) => Ok(address),
            MixedAddress::Offchain(_) => Err(MixedAddressError::NotEvmAddress),
        }
    }
}

impl Display for MixedAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MixedAddress::Evm(address) => write!(f, "{}", address),
            MixedAddress::Offchain(address) => write!(f, "{}", address),
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
        let evm_address = EvmAddress::from_str(&s);
        match evm_address {
            Ok(address) => Ok(MixedAddress::Evm(address)),
            Err(_) => {
                if OFFCHAIN_ADDRESS_REGEX.is_match(&s) {
                    Ok(MixedAddress::Offchain(s))
                } else {
                    Err(serde::de::Error::custom("Invalid address format"))
                }
            }
        }
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
        }
    }
}

/// A 32-byte EVM transaction hash, encoded as 0x-prefixed hex string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionHash(pub [u8; 32]);

impl<'de> Deserialize<'de> for TransactionHash {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;

        static TX_HASH_REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^0x[0-9a-fA-F]{64}$").expect("invalid regex"));

        if !TX_HASH_REGEX.is_match(&s) {
            return Err(serde::de::Error::custom("Invalid transaction hash format"));
        }

        let bytes = hex::decode(s.trim_start_matches("0x"))
            .map_err(|_| serde::de::Error::custom("Invalid hex in transaction hash"))?;

        let array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| serde::de::Error::custom("Transaction hash must be exactly 32 bytes"))?;

        Ok(TransactionHash(array))
    }
}

impl Serialize for TransactionHash {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let hex_string = format!("0x{}", hex::encode(self.0));
        serializer.serialize_str(&hex_string)
    }
}

impl Display for TransactionHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.0))
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

impl PaymentRequirements {
    /// Returns the [`TokenAsset`] that identifies the token required for payment.
    ///
    /// This includes the ERC-20 contract address and the associated network.
    /// It can be used for comparisons, lookups, or matching against maximum allowed token amounts.
    ///
    /// # Panics
    ///
    /// Panics if the internal `asset` field cannot be converted into an [`EvmAddress`].
    /// This should not occur if `asset` was originally derived from a valid address.
    ///
    /// # Example
    /// ```
    /// use x402_rs::types::{PaymentRequirements, TokenAsset};
    ///
    /// let reqs: PaymentRequirements = /* from parsed response or constructed */;
    /// let token: TokenAsset = reqs.token_asset();
    /// ```
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn token_asset(&self) -> TokenAsset {
        TokenAsset {
            address: self.asset.clone().try_into().unwrap(),
            network: self.network,
        }
    }
}

/// Wrapper for a payment payload and requirements sent by the client to a facilitator
/// to be verified.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest {
    pub x402_version: X402Version,
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

/// Wrapper for a payment payload and requirements sent by the client
/// to be used for settlement.
pub type SettleRequest = VerifyRequest;

#[derive(Debug, Clone, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "camelCase")]
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

/// Error returned when encoding a [`SettleResponse`] into base64 fails.
///
/// This typically occurs if the response cannot be serialized to JSON,
/// which is a prerequisite for base64 encoding in the x402 protocol.
#[derive(Debug)]
pub struct SettleResponseB64EncodingError(pub serde_json::Error);

impl Display for SettleResponseB64EncodingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Failed to encode settle response as base64 string {}",
            self.0
        )
    }
}

impl TryInto<Base64Bytes<'static>> for SettleResponse {
    type Error = SettleResponseB64EncodingError;

    fn try_into(self) -> Result<Base64Bytes<'static>, Self::Error> {
        let json = serde_json::to_vec(&self).map_err(SettleResponseB64EncodingError)?;
        Ok(Base64Bytes::encode(json))
    }
}

/// Result returned by a facilitator after verifying a [`PaymentPayload`] against the provided [`PaymentRequirements`].
///
/// This response indicates whether the payment authorization is valid and identifies the payer. If invalid,
/// it includes a reason describing why verification failed (e.g., wrong network, an invalid scheme, insufficient funds).
#[derive(Debug, Clone)]
pub enum VerifyResponse {
    /// The payload matches the requirements and passes all checks.
    Valid { payer: EvmAddress },
    /// The payload was well-formed but failed verification due to the specified [`FacilitatorErrorReason`]
    Invalid {
        reason: FacilitatorErrorReason,
        payer: EvmAddress,
    },
}

impl VerifyResponse {
    /// Constructs a successful verification response with the given `payer` address.
    ///
    /// Indicates that the provided payment payload has been validated against the payment requirements.
    pub fn valid(payer: EvmAddress) -> Self {
        VerifyResponse::Valid { payer }
    }

    /// Constructs a failed verification response with the given `payer` address and error `reason`.
    ///
    /// Indicates that the payment was recognized but rejected due to reasons such as
    /// insufficient funds, invalid network, or scheme mismatch.
    pub fn invalid(payer: EvmAddress, reason: FacilitatorErrorReason) -> Self {
        VerifyResponse::Invalid { reason, payer }
    }
}

impl Serialize for VerifyResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut s = match self {
            VerifyResponse::Valid { .. } => serializer.serialize_struct("VerifyResponse", 2)?,
            VerifyResponse::Invalid { .. } => serializer.serialize_struct("VerifyResponse", 3)?,
        };

        match self {
            VerifyResponse::Valid { payer } => {
                s.serialize_field("isValid", &true)?;
                s.serialize_field("payer", payer)?;
            }
            VerifyResponse::Invalid { reason, payer } => {
                s.serialize_field("isValid", &false)?;
                s.serialize_field("invalidReason", reason)?;
                s.serialize_field("payer", payer)?;
            }
        }

        s.end()
    }
}

impl<'de> Deserialize<'de> for VerifyResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Raw {
            is_valid: bool,
            payer: EvmAddress,
            #[serde(default)]
            invalid_reason: Option<FacilitatorErrorReason>,
        }

        let raw = Raw::deserialize(deserializer)?;

        match (raw.is_valid, raw.invalid_reason) {
            (true, None) => Ok(VerifyResponse::Valid { payer: raw.payer }),
            (false, Some(reason)) => Ok(VerifyResponse::Invalid {
                payer: raw.payer,
                reason,
            }),
            (true, Some(_)) => Err(serde::de::Error::custom(
                "`invalidReason` must be absent when `isValid` is true",
            )),
            (false, None) => Err(serde::de::Error::custom(
                "`invalidReason` must be present when `isValid` is false",
            )),
        }
    }
}

/// A simple error structure returned on unexpected or fatal server errors.
/// Used when no structured protocol-level response is appropriate.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub error: String,
}

/// Contains bytes of base64 encoded some other bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base64Bytes<'a>(pub Cow<'a, [u8]>);

impl Base64Bytes<'_> {
    /// Decode base64 string bytes to raw binary payload.
    pub fn decode(&self) -> Result<Vec<u8>, base64::DecodeError> {
        b64.decode(&self.0)
    }

    /// Encode raw binary input into base64 string bytes
    pub fn encode<T: AsRef<[u8]>>(input: T) -> Base64Bytes<'static> {
        let encoded = b64.encode(input.as_ref());
        Base64Bytes(Cow::Owned(encoded.into_bytes()))
    }
}

impl AsRef<[u8]> for Base64Bytes<'_> {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl<'a> From<&'a [u8]> for Base64Bytes<'a> {
    fn from(slice: &'a [u8]) -> Self {
        Base64Bytes(Cow::Borrowed(slice))
    }
}

/// Represents a price-like numeric value in human-readable currency format.
/// Accepts strings like "$0.01", "1,000", "€20", or raw numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct MoneyAmount(pub Decimal);

impl MoneyAmount {
    /// Returns the number of digits after the decimal point in the original input.
    ///
    /// This is useful for checking precision constraints when converting
    /// human-readable amounts (e.g., `$0.01`) to on-chain token values.
    pub fn scale(&self) -> u32 {
        self.0.scale()
    }

    /// Returns the absolute mantissa of the decimal value as an unsigned integer.
    ///
    /// For example, the mantissa of `-12.34` is `1234`.
    /// Used when scaling values to match token decimal places.
    pub fn mantissa(&self) -> u128 {
        self.0.mantissa().unsigned_abs()
    }

    /// Converts the [`MoneyAmount`] into a raw on-chain [`TokenAmount`] by scaling
    /// the mantissa to match a given token's decimal precision.
    ///
    /// For example, `$0.01` becomes `10000` when targeting a token with 6 decimals.
    ///
    /// Returns an error if the precision of the money amount exceeds the allowed token precision,
    /// to prevent unintentional truncation or rounding errors.
    ///
    /// This method is useful for converting user-input values like `"0.01"` into
    /// canonical [`U256`] token amounts that are expected in protocol-layer messages.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn as_token_amount(
        &self,
        token_decimals: u32,
    ) -> Result<TokenAmount, MoneyAmountParseError> {
        let money_amount = self;
        let money_decimals = money_amount.scale();
        if money_decimals > token_decimals {
            return Err(MoneyAmountParseError::WrongPrecision {
                money: money_decimals,
                token: token_decimals,
            });
        }
        let scale_diff = token_decimals - money_decimals;
        let multiplier = U256::from(10).pow(U256::from(scale_diff));
        let digits = money_amount.mantissa();
        let value = U256::from(digits).mul(multiplier);
        Ok(value)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MoneyAmountParseError {
    #[error("Invalid number format")]
    InvalidFormat,
    #[error(
        "Amount must be between {} and {}",
        money_amount::MIN_STR,
        money_amount::MAX_STR
    )]
    OutOfRange,
    #[error("Negative value is not allowed")]
    Negative,
    #[error("Too big of a precision: {money} vs {token} on token")]
    WrongPrecision { money: u32, token: u32 },
}

mod money_amount {
    use super::*;

    pub const MIN_STR: &str = "0.000000001";
    pub const MAX_STR: &str = "999999999";

    pub static MIN: Lazy<Decimal> =
        Lazy::new(|| Decimal::from_str(MIN_STR).expect("valid decimal"));
    pub static MAX: Lazy<Decimal> =
        Lazy::new(|| Decimal::from_str(MAX_STR).expect("valid decimal"));
}

impl MoneyAmount {
    pub fn parse(input: &str) -> Result<Self, MoneyAmountParseError> {
        // Remove anything that isn't digit, dot, minus
        let cleaned = Regex::new(r"[^\d\.\-]+")
            .unwrap()
            .replace_all(input, "")
            .to_string();

        let parsed =
            Decimal::from_str(&cleaned).map_err(|_| MoneyAmountParseError::InvalidFormat)?;

        if parsed.is_sign_negative() {
            return Err(MoneyAmountParseError::Negative);
        }

        if parsed < *money_amount::MIN || parsed > *money_amount::MAX {
            return Err(MoneyAmountParseError::OutOfRange);
        }

        Ok(MoneyAmount(parsed))
    }
}

impl FromStr for MoneyAmount {
    type Err = MoneyAmountParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        MoneyAmount::parse(s)
    }
}

impl TryFrom<&str> for MoneyAmount {
    type Error = MoneyAmountParseError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        MoneyAmount::from_str(value)
    }
}

impl From<u128> for MoneyAmount {
    fn from(value: u128) -> Self {
        MoneyAmount(Decimal::from(value))
    }
}

impl TryFrom<f64> for MoneyAmount {
    type Error = MoneyAmountParseError;

    fn try_from(value: f64) -> Result<Self, Self::Error> {
        let decimal = Decimal::from_f64(value).ok_or(MoneyAmountParseError::OutOfRange)?;
        if decimal.is_sign_negative() {
            return Err(MoneyAmountParseError::Negative);
        }
        if decimal < *money_amount::MIN || decimal > *money_amount::MAX {
            return Err(MoneyAmountParseError::OutOfRange);
        }
        Ok(MoneyAmount(decimal))
    }
}

impl Display for MoneyAmount {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0.normalize())
    }
}

/// Metadata required to identify a token in EIP-712 typed data signatures.
///
/// This struct contains the `name` and `version` fields used in the EIP-712 domain separator,
/// as required when signing `transferWithAuthorization` messages for ERC-3009-compatible tokens.
///
/// These values must match exactly what the token contract returns from `name()` and `version()`
/// and are critical for ensuring signature validity and replay protection across different token versions.
///
/// Used in conjunction with [`TokenDeployment`] to define a token asset for payment authorization.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TokenDeploymentEip712 {
    pub name: String,
    pub version: String,
}

/// Represents a fungible token identified by its address and network,
/// used for selecting or matching assets across chains (e.g., USDC on Base).
///
/// This struct does not include metadata like `decimals` or EIP-712 signing info.
///
/// # Example
///
/// ```
/// use x402_rs::types::{TokenAsset, EvmAddress};
/// use x402_rs::network::Network;
///
/// let asset = TokenAsset {
///     address: "0x036CbD53842c5426634e7929541eC2318f3dCF7e".parse().unwrap(),
///     network: Network::BaseSepolia,
/// };
///
/// assert_eq!(asset.address.to_string(), "0x036CbD53842c5426634e7929541eC2318f3dCF7e");
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TokenAsset {
    pub address: EvmAddress,
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub network: Network,
}

impl From<TokenAsset> for Vec<TokenAsset> {
    fn from(asset: TokenAsset) -> Vec<TokenAsset> {
        vec![asset]
    }
}

impl Display for TokenAsset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Use CAIP-19 https://chainagnostic.org/CAIPs/caip-19
        write!(
            f,
            "eip155:{}/erc20:{}",
            self.network.chain_id(),
            self.address
        )
    }
}

/// Describes a specific deployed ERC-20 token instance, including metadata
/// required for value formatting and EIP-712 signing.
///
/// This is the canonical representation used when signing `TransferWithAuthorization`.
///
/// # Example
///
/// ```
/// use x402_rs::types::{TokenAsset, TokenDeployment, TokenDeploymentEip712};
/// use x402_rs::network::Network;
///
/// let asset = TokenAsset {
///     address: "0x036CbD53842c5426634e7929541eC2318f3dCF7e".parse().unwrap(),
///     network: Network::BaseSepolia,
/// };
///
/// let deployment = TokenDeployment {
///     asset,
///     decimals: 6,
///     eip712: TokenDeploymentEip712 {
///         name: "MyToken".into(),
///         version: "1".into(),
///     },
/// };
///
/// assert_eq!(deployment.asset.address.to_string(), "0x036CbD53842c5426634e7929541eC2318f3dCF7e");
/// assert_eq!(deployment.decimals, 6);
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct TokenDeployment {
    pub asset: TokenAsset,
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub decimals: u8,
    pub eip712: TokenDeploymentEip712,
}

impl TokenDeployment {
    pub fn address(&self) -> EvmAddress {
        self.asset.address
    }

    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn network(&self) -> Network {
        self.asset.network
    }
}

impl From<TokenDeployment> for Vec<TokenAsset> {
    fn from(value: TokenDeployment) -> Self {
        vec![value.asset]
    }
}

impl From<TokenDeployment> for TokenAsset {
    fn from(value: TokenDeployment) -> Self {
        value.asset
    }
}

/// Response returned from an x402 payment-gated endpoint when no valid payment was provided or accepted.
///
/// This structure informs the client that payment is required to proceed and communicates:
/// - an `error` message describing the reason (e.g., missing header, invalid format, no matching requirements),
/// - a list of acceptable [`PaymentRequirements`],
/// - an optional `payer` address if one could be extracted from a failed verification/settlement,
/// - and the `x402_version` to indicate protocol compatibility.
///
/// This type is serialized into an HTTP 402 ("Payment Required") response and consumed by clients implementing the x402 protocol.
///
/// It may be returned in the following cases (not exhaustive):
/// - Missing `X-Payment` header
/// - Malformed or unverifiable payment payload
/// - No matching payment requirements found
/// - Verification or settlement failed
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequiredResponse {
    pub error: String,
    pub accepts: Vec<PaymentRequirements>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payer: Option<EvmAddress>,
    pub x402_version: X402Version,
}

impl Display for PaymentRequiredResponse {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PaymentRequiredResponse: error='{}', accepts={} requirement(s), payer={}, version={}",
            self.error,
            self.accepts.len(),
            self.payer
                .as_ref()
                .map(ToString::to_string)
                .unwrap_or_else(|| "unknown".to_string()),
            self.x402_version
        )
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedPaymentKind {
    pub x402_version: X402Version,
    pub scheme: Scheme,
    pub network: Network,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedPaymentKindsResponse {
    pub kinds: Vec<SupportedPaymentKind>,
}

sol!(
    /// Solidity-compatible struct definition for ERC-3009 `transferWithAuthorization`.
    ///
    /// This matches the EIP-3009 format used in EIP-712 typed data:
    /// it defines the authorization to transfer tokens from `from` to `to`
    /// for a specific `value`, valid only between `validAfter` and `validBefore`
    /// and identified by a unique `nonce`.
    ///
    /// This struct is primarily used to reconstruct the typed data domain/message
    /// when verifying a client's signature.
    #[derive(Serialize, Deserialize)]
    struct TransferWithAuthorization {
        address from;
        address to;
        uint256 value;
        uint256 validAfter;
        uint256 validBefore;
        bytes32 nonce;
    }
);
