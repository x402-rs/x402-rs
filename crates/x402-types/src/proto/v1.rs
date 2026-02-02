//! Protocol version 1 (V1) types for x402.
//!
//! This module defines the wire format types for the original x402 protocol version.
//! V1 uses network names (e.g., "base-sepolia") instead of CAIP-2 chain IDs.
//!
//! # Key Types
//!
//! - [`X402Version1`] - Version marker that serializes as `1`
//! - [`PaymentPayload`] - Signed payment authorization from the buyer
//! - [`PaymentRequirements`] - Payment terms set by the seller
//! - [`PaymentRequired`] - HTTP 402 response body
//! - [`VerifyRequest`] / [`VerifyResponse`] - Verification messages
//! - [`SettleResponse`] - Settlement result
//! - [`PriceTag`] - Builder for creating payment requirements

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fmt::Display;
use std::str::FromStr;
use std::sync::Arc;

use crate::proto;
use crate::proto::SupportedResponse;

/// Version marker for x402 protocol version 1.
///
/// This type serializes as the integer `1` and is used to identify V1 protocol
/// messages in the wire format.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct X402Version1;

impl X402Version1 {
    pub const VALUE: u8 = 1;
}

impl PartialEq<u8> for X402Version1 {
    fn eq(&self, other: &u8) -> bool {
        *other == Self::VALUE
    }
}

impl From<X402Version1> for u8 {
    fn from(_: X402Version1) -> Self {
        X402Version1::VALUE
    }
}

impl Serialize for X402Version1 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(Self::VALUE)
    }
}

impl<'de> Deserialize<'de> for X402Version1 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let num = u8::deserialize(deserializer)?;
        if num == Self::VALUE {
            Ok(X402Version1)
        } else {
            Err(serde::de::Error::custom(format!(
                "expected version {}, got {}",
                Self::VALUE,
                num
            )))
        }
    }
}

impl Display for X402Version1 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", Self::VALUE)
    }
}

/// Response from a payment settlement request.
///
/// Indicates whether the payment was successfully settled on-chain.
pub enum SettleResponse {
    /// Settlement succeeded.
    Success {
        /// The address that paid.
        payer: String,
        /// The transaction hash.
        transaction: String,
        /// The network where settlement occurred.
        network: String,
    },
    /// Settlement failed.
    Error {
        /// The reason for failure.
        reason: String,
        /// The network where settlement was attempted.
        network: String,
    },
}

impl From<SettleResponse> for proto::SettleResponse {
    fn from(val: SettleResponse) -> Self {
        proto::SettleResponse(
            serde_json::to_value(val).expect("SettleResponse serialization failed"),
        )
    }
}

#[derive(Serialize, Deserialize)]
struct SettleResponseWire {
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transaction: Option<String>,
    pub network: String,
}

impl Serialize for SettleResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match self {
            SettleResponse::Success {
                payer,
                transaction,
                network,
            } => SettleResponseWire {
                success: true,
                error_reason: None,
                payer: Some(payer.clone()),
                transaction: Some(transaction.clone()),
                network: network.clone(),
            },
            SettleResponse::Error { reason, network } => SettleResponseWire {
                success: false,
                error_reason: Some(reason.clone()),
                payer: None,
                transaction: None,
                network: network.clone(),
            },
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for SettleResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = SettleResponseWire::deserialize(deserializer)?;
        match wire.success {
            true => {
                let payer = wire
                    .payer
                    .ok_or_else(|| serde::de::Error::missing_field("payer"))?;
                let transaction = wire
                    .transaction
                    .ok_or_else(|| serde::de::Error::missing_field("transaction"))?;
                Ok(SettleResponse::Success {
                    payer,
                    transaction,
                    network: wire.network,
                })
            }
            false => {
                let reason = wire
                    .error_reason
                    .ok_or_else(|| serde::de::Error::missing_field("error_reason"))?;
                Ok(SettleResponse::Error {
                    reason,
                    network: wire.network,
                })
            }
        }
    }
}

/// Result returned by a facilitator after verifying a [`PaymentPayload`] against the provided [`PaymentRequirements`].
///
/// This response indicates whether the payment authorization is valid and identifies the payer. If invalid,
/// it includes a reason describing why verification failed (e.g., wrong network, an invalid scheme, insufficient funds).
#[derive(Debug)]
pub enum VerifyResponse {
    /// The payload matches the requirements and passes all checks.
    Valid { payer: String },
    /// The payload was well-formed but failed verification due to the specified [`FacilitatorErrorReason`]
    Invalid {
        reason: String,
        payer: Option<String>,
    },
}

impl From<VerifyResponse> for proto::VerifyResponse {
    fn from(val: VerifyResponse) -> Self {
        proto::VerifyResponse(
            serde_json::to_value(val).expect("VerifyResponse serialization failed"),
        )
    }
}

impl TryFrom<proto::VerifyResponse> for VerifyResponse {
    type Error = serde_json::Error;
    fn try_from(value: proto::VerifyResponse) -> Result<Self, Self::Error> {
        let json = value.0;
        serde_json::from_value(json)
    }
}

impl VerifyResponse {
    /// Constructs a successful verification response with the given `payer` address.
    ///
    /// Indicates that the provided payment payload has been validated against the payment requirements.
    pub fn valid(payer: String) -> Self {
        VerifyResponse::Valid { payer }
    }

    /// Constructs a failed verification response with the given `payer` address and error `reason`.
    ///
    /// Indicates that the payment was recognized but rejected due to reasons such as
    /// insufficient funds, invalid network, or scheme mismatch.
    #[allow(dead_code)]
    pub fn invalid(payer: Option<String>, reason: String) -> Self {
        VerifyResponse::Invalid { reason, payer }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct VerifyResponseWire {
    is_valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    payer: Option<String>,
    #[serde(default)]
    invalid_reason: Option<String>,
}

impl Serialize for VerifyResponse {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let wire = match self {
            VerifyResponse::Valid { payer } => VerifyResponseWire {
                is_valid: true,
                payer: Some(payer.clone()),
                invalid_reason: None,
            },
            VerifyResponse::Invalid { reason, payer } => VerifyResponseWire {
                is_valid: false,
                payer: payer.clone(),
                invalid_reason: Some(reason.clone()),
            },
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for VerifyResponse {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let wire = VerifyResponseWire::deserialize(deserializer)?;
        match wire.is_valid {
            true => {
                let payer = wire
                    .payer
                    .ok_or_else(|| serde::de::Error::missing_field("payer"))?;
                Ok(VerifyResponse::Valid { payer })
            }
            false => {
                let reason = wire
                    .invalid_reason
                    .ok_or_else(|| serde::de::Error::missing_field("invalid_reason"))?;
                let payer = wire.payer;
                Ok(VerifyResponse::Invalid { reason, payer })
            }
        }
    }
}

/// Request to verify a V1 payment.
///
/// Contains the payment payload and requirements for verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest<TPayload, TRequirements> {
    /// Protocol version (always 1).
    pub x402_version: X402Version1,
    /// The signed payment authorization.
    pub payment_payload: TPayload,
    /// The payment requirements to verify against.
    pub payment_requirements: TRequirements,
}

impl<TPayload, TRequirements> VerifyRequest<TPayload, TRequirements>
where
    Self: DeserializeOwned,
{
    pub fn from_proto(
        request: proto::VerifyRequest,
    ) -> Result<Self, proto::PaymentVerificationError> {
        let deserialized: Self = serde_json::from_value(request.into_json())?;
        Ok(deserialized)
    }
}

impl<TPayload, TRequirements> TryInto<proto::VerifyRequest>
    for VerifyRequest<TPayload, TRequirements>
where
    TPayload: Serialize,
    TRequirements: Serialize,
{
    type Error = serde_json::Error;
    fn try_into(self) -> Result<proto::VerifyRequest, Self::Error> {
        let json = serde_json::to_value(self)?;
        Ok(proto::VerifyRequest(json))
    }
}

/// A signed payment authorization from the buyer.
///
/// This contains the cryptographic proof that the buyer has authorized
/// a payment, along with metadata about the payment scheme and network.
///
/// # Type Parameters
///
/// - `TScheme` - The scheme identifier type (default: `String`)
/// - `TPayload` - The scheme-specific payload type (default: raw JSON)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload<TScheme = String, TPayload = Box<serde_json::value::RawValue>> {
    /// Protocol version (always 1).
    pub x402_version: X402Version1,
    /// The payment scheme (e.g., "exact").
    pub scheme: TScheme,
    /// The network name (e.g., "base-sepolia").
    pub network: String,
    /// The scheme-specific signed payload.
    pub payload: TPayload,
}

/// Payment requirements set by the seller.
///
/// Defines the terms under which a payment will be accepted, including
/// the amount, recipient, asset, and timing constraints.
///
/// # Type Parameters
///
/// - `TScheme` - The scheme identifier type (default: `String`)
/// - `TAmount` - The amount type (default: `String`)
/// - `TAddress` - The address type (default: `String`)
/// - `TExtra` - Scheme-specific extra data type (default: `serde_json::Value`)
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements<
    TScheme = String,
    TAmount = String,
    TAddress = String,
    TExtra = serde_json::Value,
> {
    /// The payment scheme (e.g., "exact").
    pub scheme: TScheme,
    /// The network name (e.g., "base-sepolia").
    pub network: String,
    /// The maximum amount required for payment.
    pub max_amount_required: TAmount,
    /// The resource URL being paid for.
    pub resource: String,
    /// Human-readable description of the resource.
    pub description: String,
    /// MIME type of the resource.
    pub mime_type: String,
    /// Optional JSON schema for the resource output.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
    /// The recipient address for payment.
    pub pay_to: TAddress,
    /// Maximum time in seconds for payment validity.
    pub max_timeout_seconds: u64,
    /// The token asset address.
    pub asset: TAddress,
    /// Scheme-specific extra data.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<TExtra>,
}

impl PaymentRequirements {
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn as_concrete<
        TScheme: FromStr,
        TAmount: FromStr,
        TAddress: FromStr,
        TExtra: DeserializeOwned,
    >(
        &self,
    ) -> Option<PaymentRequirements<TScheme, TAmount, TAddress, TExtra>> {
        let scheme = self.scheme.parse::<TScheme>().ok()?;
        let max_amount_required = self.max_amount_required.parse::<TAmount>().ok()?;
        let pay_to = self.pay_to.parse::<TAddress>().ok()?;
        let asset = self.asset.parse::<TAddress>().ok()?;
        let extra = self
            .extra
            .as_ref()
            .and_then(|v| serde_json::from_value(v.clone()).ok());
        Some(PaymentRequirements {
            scheme,
            network: self.network.clone(),
            max_amount_required,
            resource: self.resource.clone(),
            description: self.description.clone(),
            mime_type: self.mime_type.clone(),
            output_schema: self.output_schema.clone(),
            pay_to,
            max_timeout_seconds: self.max_timeout_seconds,
            asset,
            extra,
        })
    }
}

/// HTTP 402 Payment Required response body for V1.
///
/// This is returned when a resource requires payment. It contains
/// the list of acceptable payment methods.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequired {
    /// Protocol version (always 1).
    pub x402_version: X402Version1,
    /// List of acceptable payment methods.
    #[serde(default)]
    pub accepts: Vec<PaymentRequirements>,
    /// Optional error message if the request was malformed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Builder for creating payment requirements.
///
/// A `PriceTag` is a convenient way to specify payment terms that can
/// be converted into [`PaymentRequirements`] for inclusion in a 402 response.
///
/// # Example
///
/// ```rust
/// use x402_types::proto::v1::PriceTag;
///
/// let price = PriceTag {
///     scheme: "exact".to_string(),
///     pay_to: "0x1234...".to_string(),
///     asset: "0xUSDC...".to_string(),
///     network: "base".to_string(),
///     amount: "1000000".to_string(), // 1 USDC
///     max_timeout_seconds: 300,
///     extra: None,
///     enricher: None,
/// };
/// ```
#[derive(Clone)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct PriceTag {
    /// The payment scheme (e.g., "exact").
    pub scheme: String,
    /// The recipient address.
    pub pay_to: String,
    /// The token asset address.
    pub asset: String,
    /// The network name.
    pub network: String,
    /// The payment amount in token units.
    pub amount: String,
    /// Maximum time in seconds for payment validity.
    pub max_timeout_seconds: u64,
    /// Scheme-specific extra data.
    pub extra: Option<serde_json::Value>,
    /// Optional enrichment function for adding facilitator-specific data.
    #[doc(hidden)]
    pub enricher: Option<Enricher>,
}

impl fmt::Debug for PriceTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PriceTag")
            .field("scheme", &self.scheme)
            .field("pay_to", &self.pay_to)
            .field("asset", &self.asset)
            .field("network", &self.network)
            .field("amount", &self.amount)
            .field("max_timeout_seconds", &self.max_timeout_seconds)
            .field("extra", &self.extra)
            .finish()
    }
}

/// Enrichment function type for price tags.
///
/// Enrichers are called with the facilitator's capabilities to add
/// facilitator-specific data to price tags (e.g., fee payer addresses).
pub type Enricher = Arc<dyn Fn(&mut PriceTag, &SupportedResponse) + Send + Sync>;

impl PriceTag {
    /// Applies the enrichment function if one is set.
    ///
    /// This is called automatically when building payment requirements
    /// to add facilitator-specific data.
    #[allow(dead_code)]
    pub fn enrich(&mut self, capabilities: &SupportedResponse) {
        if let Some(enricher) = self.enricher.clone() {
            enricher(self, capabilities);
        }
    }

    /// Sets the maximum timeout for this price tag.
    #[allow(dead_code)]
    pub fn with_timeout(mut self, seconds: u64) -> Self {
        self.max_timeout_seconds = seconds;
        self
    }
}
