//! Type definitions for the x402 protocol as used by this facilitator.
//!
//! This mirrors the structures and validation logic from official x402 SDKs (TypeScript/Go).
//! The key objects are `PaymentPayload`, `PaymentRequirements`, `VerifyResponse`, and `SettleResponse`,
//! which encode payment intent, authorization, and the result of verification/settlement.
//!
//! This module supports ERC-3009 style authorization for tokens (EIP-712 typed signatures),
//! and provides serialization logic compatible with external clients.

use alloy::hex::FromHex;
use alloy::primitives::{AddressError, U256};
use alloy::{hex, sol};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::de::Error;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fmt::Display;
use url::Url;

use crate::network::Network;

pub const EVM_MAX_ATOMIC_UNITS: usize = 18;

/// Represents the protocol version. Currently only version 1 is supported.
#[derive(Debug, Copy, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum X402Version {
    #[serde(rename = "1")]
    V1,
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

impl<'de> serde::Deserialize<'de> for X402Version {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let num = u8::deserialize(deserializer)?;
        X402Version::try_from(num).map_err(Error::custom)
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

/// Represents a 65-byte ECDSA signature used in EIP-712 typed data.
/// Serialized as 0x-prefixed hex string with 130 characters.
/// Used to authorize an ERC-3009 transferWithAuthorization.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct EvmSignature(pub [u8; 65]);

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
            let bytes = hex::decode(s.trim_start_matches("0x"))
                .map_err(|_| Error::custom("Failed to decode EVM signature hex string"))?;

            let array: [u8; 65] = bytes
                .try_into()
                .map_err(|_| Error::custom("Signature must be exactly 65 bytes"))?;

            Ok(EvmSignature(array))
        } else {
            Err(Error::custom(
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

/// Wrapper around `alloy::primitives::Address`, providing display/serialization support.
/// Used throughout the protocol for typed Ethereum address handling.
#[derive(Debug, Copy, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvmAddress(pub alloy::primitives::Address);

impl Display for EvmAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<EvmAddress> for alloy::primitives::Address {
    fn from(address: EvmAddress) -> Self {
        address.0
    }
}

impl From<EvmAddress> for MixedAddress {
    fn from(address: EvmAddress) -> Self {
        MixedAddress(format!("{}", address))
    }
}

/// Represents a 32-byte random nonce, hex-encoded with 0x prefix.
/// Must be exactly 64 hex characters long.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct HexEncodedNonce(pub [u8; 32]);

impl<'de> Deserialize<'de> for HexEncodedNonce {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        static NONCE_REGEX: Lazy<Regex> =
            Lazy::new(|| Regex::new(r"^0x[0-9a-fA-F]{64}$").expect("Invalid nonce regex"));

        if !NONCE_REGEX.is_match(&s) {
            return Err(Error::custom("Invalid nonce format"));
        }

        let bytes = hex::decode(&s[2..]).map_err(|_| Error::custom("Invalid hex in nonce"))?;

        let array: [u8; 32] = bytes
            .try_into()
            .map_err(|_| Error::custom("Invalid length for nonce"))?;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ExactEvmPayloadValue(pub u64);

impl<'de> Deserialize<'de> for ExactEvmPayloadValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        if s.len() > EVM_MAX_ATOMIC_UNITS {
            return Err(serde::de::Error::custom(format!(
                "Value too long (max {} digits)",
                EVM_MAX_ATOMIC_UNITS
            )));
        }

        let value = s.parse::<u64>().map_err(|_| {
            serde::de::Error::custom("Value is not a valid non-negative integer fitting u64")
        })?;

        Ok(ExactEvmPayloadValue(value))
    }
}

impl From<ExactEvmPayloadValue> for U256 {
    fn from(value: ExactEvmPayloadValue) -> Self {
        U256::from(value.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct UnixTimestamp(pub u64);

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
    pub value: ExactEvmPayloadValue,
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
    #[allow(dead_code)]
    pub x402_version: X402Version,
    pub scheme: Scheme,
    pub network: Network,
    pub payload: ExactEvmPayload,
}

/// The maximum token amount a user is required to pay, expressed as a `u64`.
/// Parsed from string to prevent accidental loss of precision in JSON serialization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct MaxAmountRequired(pub u64);

impl<'de> Deserialize<'de> for MaxAmountRequired {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        String::deserialize(deserializer)
            .and_then(|string| string.parse::<u64>().map_err(Error::custom))
            .map(MaxAmountRequired)
    }
}

/// Represents either an EVM address (0x...) or an off-chain address.
/// Format is validated by regex and used for routing settlement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MixedAddress(pub String);

impl TryInto<alloy::primitives::Address> for MixedAddress {
    type Error = AddressError;

    fn try_into(self) -> Result<alloy::primitives::Address, Self::Error> {
        Ok(alloy::primitives::Address::from_hex(self.0)?)
    }
}

impl TryInto<EvmAddress> for MixedAddress {
    type Error = AddressError;
    fn try_into(self) -> Result<EvmAddress, Self::Error> {
        let address: alloy::primitives::Address = self.try_into()?;
        Ok(EvmAddress(address))
    }
}

impl Display for MixedAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl<'de> Deserialize<'de> for MixedAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        static MIXED_ADDRESS_REGEX: Lazy<Regex> = Lazy::new(|| {
            Regex::new(r"^(0x[a-fA-F0-9]{40}|[A-Za-z0-9][A-Za-z0-9-]{0,34}[A-Za-z0-9])$")
                .expect("Invalid MixedAddress regex")
        });

        if MIXED_ADDRESS_REGEX.is_match(&s) {
            Ok(MixedAddress(s))
        } else {
            Err(serde::de::Error::custom("Invalid MixedAddress format"))
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

/// Requirements set by the verifier for an acceptable payment.
/// This includes min/max amounts, recipient, asset, network, and metadata.
#[derive(Debug, Serialize, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements {
    pub scheme: Scheme,
    pub network: Network,
    pub max_amount_required: MaxAmountRequired,
    pub resource: Url,
    pub description: String,
    pub mime_type: String,
    pub output_schema: Option<serde_json::Value>,
    pub pay_to: MixedAddress,
    pub max_timeout_seconds: u64,
    pub asset: MixedAddress,
    pub extra: Option<serde_json::Value>,
}

/// Wrapper for a payment payload and requirements sent by the client
/// to be verified.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest {
    pub payment_payload: PaymentPayload,
    pub payment_requirements: PaymentRequirements,
}

impl std::fmt::Display for VerifyRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "VerifyRequest(payment_payload={:?}, payment_requirements={:?})",
            self.payment_payload, self.payment_requirements
        )
    }
}

/// Wrapper for a payment payload and requirements sent by the client
/// to be used for settlement.
pub type SettleRequest = VerifyRequest;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorReason {
    #[serde(rename = "insufficient_funds")]
    InsufficientFunds,
    #[serde(rename = "invalid_scheme")]
    InvalidScheme,
    #[serde(rename = "invalid_network")]
    InvalidNetwork,
}

/// Returned after attempting to settle a payment on-chain.
/// Indicates success/failure, transaction hash, and payer identity.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettleResponse {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<ErrorReason>,
    pub payer: MixedAddress,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<TransactionHash>,
    pub network: Network,
}

/// Returned after verifying a `PaymentPayload` against `PaymentRequirements`.
/// Includes a boolean flag and an optional error reason.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyResponse {
    pub is_valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub invalid_reason: Option<ErrorReason>,
    pub payer: EvmAddress,
}

/// A simple error structure returned on unexpected or fatal server errors.
/// Used when no structured protocol-level response is appropriate.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorResponse {
    pub error: String,
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
