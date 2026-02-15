//! Protocol version 2 (V2) types for x402.
//!
//! This module defines the wire format types for the enhanced x402 protocol version.
//! V2 uses CAIP-2 chain IDs (e.g., "eip155:8453") instead of network names, and
//! includes richer resource metadata.
//!
//! # Key Differences from V1
//!
//! - Uses CAIP-2 chain IDs instead of network names
//! - Includes [`ResourceInfo`] with URL, description, and MIME type
//! - Simplified [`PaymentRequirements`] structure
//! - Payment payload includes accepted requirements for verification
//!
//! # Key Types
//!
//! - [`X402Version2`] - Version marker that serializes as `2`
//! - [`PaymentPayload`] - Signed payment with accepted requirements
//! - [`PaymentRequirements`] - Payment terms set by the seller
//! - [`PaymentRequired`] - HTTP 402 response body
//! - [`ResourceInfo`] - Metadata about the paid resource
//! - [`PriceTag`] - Builder for creating payment requirements

use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::sync::Arc;

use crate::chain::ChainId;
use crate::proto;
use crate::proto::v1;
use crate::proto::{OriginalJson, SupportedResponse};

/// Version marker for x402 protocol version 2.
///
/// This type serializes as the integer `2` and is used to identify V2 protocol
/// messages in the wire format.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct X402Version2;

impl X402Version2 {
    pub const VALUE: u8 = 2;
}

impl PartialEq<u8> for X402Version2 {
    fn eq(&self, other: &u8) -> bool {
        *other == Self::VALUE
    }
}

impl From<X402Version2> for u8 {
    fn from(_: X402Version2) -> Self {
        X402Version2::VALUE
    }
}

impl Serialize for X402Version2 {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u8(Self::VALUE)
    }
}

impl<'de> Deserialize<'de> for X402Version2 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let num = u8::deserialize(deserializer)?;
        if num == Self::VALUE {
            Ok(X402Version2)
        } else {
            Err(serde::de::Error::custom(format!(
                "expected version {}, got {}",
                Self::VALUE,
                num
            )))
        }
    }
}

impl Display for X402Version2 {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", Self::VALUE)
    }
}

/// Response from a V2 payment verification request.
///
/// V2 uses the same response format as V1.
pub type VerifyResponse = v1::VerifyResponse;

/// Response from a V2 payment settlement request.
///
/// V2 uses the same response format as V1.
pub type SettleResponse = v1::SettleResponse;

/// Metadata about the resource being paid for.
///
/// This provides human-readable information about what the buyer is paying for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceInfo {
    /// Human-readable description of the resource.
    pub description: String,
    /// MIME type of the resource content.
    pub mime_type: String,
    /// URL of the resource.
    pub url: String,
}

/// Request to verify a V2 payment.
///
/// Contains the payment payload and requirements for verification.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest<TPayload, TRequirements> {
    /// Protocol version (always 2).
    pub x402_version: X402Version2,
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

/// A signed payment authorization from the buyer (V2 format).
///
/// In V2, the payment payload includes the accepted requirements, allowing
/// the facilitator to verify that the buyer agreed to specific terms.
///
/// # Type Parameters
///
/// - `TAccepted` - The accepted requirements type
/// - `TPayload` - The scheme-specific payload type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload<TPaymentRequirements, TPayload> {
    /// The payment requirements the buyer accepted.
    pub accepted: TPaymentRequirements,
    /// The scheme-specific signed payload.
    pub payload: TPayload,
    /// Information about the resource being paid for.
    pub resource: Option<ResourceInfo>,
    /// Protocol version (always 2).
    pub x402_version: X402Version2,
}

/// Payment requirements set by the seller (V2 format).
///
/// Defines the terms under which a payment will be accepted. V2 uses
/// CAIP-2 chain IDs and has a simplified structure compared to V1.
///
/// # Type Parameters
///
/// - `TScheme` - The scheme identifier type (default: `String`)
/// - `TAmount` - The amount type (default: `String`)
/// - `TAddress` - The address type (default: `String`)
/// - `TExtra` - Scheme-specific extra data type (default: `Option<serde_json::Value>`)
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements<
    TScheme = String,
    TAmount = String,
    TAddress = String,
    TExtra = Option<serde_json::Value>,
> {
    /// The payment scheme (e.g., "exact").
    pub scheme: TScheme,
    /// The CAIP-2 chain ID (e.g., "eip155:8453").
    pub network: ChainId,
    /// The payment amount in token units.
    pub amount: TAmount,
    /// The recipient address for payment.
    pub pay_to: TAddress,
    /// Maximum time in seconds for payment validity.
    pub max_timeout_seconds: u64,
    /// The token asset address.
    pub asset: TAddress,
    /// Scheme-specific extra data.
    pub extra: TExtra,
}

impl<TScheme, TAmount, TAddress, TExtra> TryFrom<&OriginalJson>
    for PaymentRequirements<TScheme, TAmount, TAddress, TExtra>
where
    TScheme: for<'a> serde::Deserialize<'a>,
    TAmount: for<'a> serde::Deserialize<'a>,
    TAddress: for<'a> serde::Deserialize<'a>,
    TExtra: for<'a> serde::Deserialize<'a>,
{
    type Error = serde_json::Error;

    fn try_from(value: &OriginalJson) -> Result<Self, Self::Error> {
        let payment_requirements = serde_json::from_str(value.0.get())?;
        Ok(payment_requirements)
    }
}

/// HTTP 402 Payment Required response body for V2.
///
/// This is returned when a resource requires payment. It contains
/// the list of acceptable payment methods and resource metadata.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequired<TAccepts = PaymentRequirements> {
    /// Protocol version (always 2).
    pub x402_version: X402Version2,
    /// Optional error message if the request was malformed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// Information about the resource being paid for.
    pub resource: ResourceInfo,
    /// List of acceptable payment methods.
    #[serde(default = "Vec::default")]
    pub accepts: Vec<TAccepts>,
}

/// Builder for creating V2 payment requirements.
///
/// A `PriceTag` wraps [`PaymentRequirements`] and provides enrichment
/// capabilities for adding facilitator-specific data.
///
/// # Example
///
/// ```rust
/// use x402_types::proto::v2::{PriceTag, PaymentRequirements};
/// use x402_types::chain::ChainId;
///
/// let requirements = PaymentRequirements {
///     scheme: "exact".to_string(),
///     network: "eip155:8453".parse().unwrap(),
///     amount: "1000000".to_string(),
///     pay_to: "0x1234...".to_string(),
///     asset: "0xUSDC...".to_string(),
///     max_timeout_seconds: 300,
///     extra: None,
/// };
///
/// let price = PriceTag {
///     requirements,
///     enricher: None,
/// };
/// ```
#[derive(Clone)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct PriceTag {
    /// The payment requirements.
    pub requirements: PaymentRequirements,
    /// Optional enrichment function for adding facilitator-specific data.
    #[doc(hidden)]
    pub enricher: Option<Enricher>,
}

impl fmt::Debug for PriceTag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PriceTag")
            .field("requirements", &self.requirements)
            .finish()
    }
}

/// Enrichment function type for V2 price tags.
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
        self.requirements.max_timeout_seconds = seconds;
        self
    }
}

/// Compares a [`PriceTag`] with [`PaymentRequirements`].
///
/// This allows checking if a price tag matches specific requirements.
impl PartialEq<PaymentRequirements> for PriceTag {
    fn eq(&self, b: &PaymentRequirements) -> bool {
        let a = &self.requirements;
        a == b
    }
}
