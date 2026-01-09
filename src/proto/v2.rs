use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;

use crate::chain::ChainId;
use crate::proto;
use crate::proto::SupportedResponse;
use crate::proto::v1;

/// Version 2 of the x402 protocol.
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

pub type VerifyResponse = v1::VerifyResponse;
pub type SettleResponse = v1::SettleResponse;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ResourceInfo {
    pub description: String,
    pub mime_type: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest<TPayload, TRequirements> {
    pub x402_version: X402Version2,
    pub payment_payload: TPayload,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayload<TAccepted, TPayload> {
    pub accepted: TAccepted,
    pub payload: TPayload,
    pub resource: ResourceInfo,
    pub x402_version: X402Version2,
}

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirements<
    TScheme = String,
    TAmount = String,
    TAddress = String,
    TExtra = Box<serde_json::value::RawValue>,
> {
    pub scheme: TScheme,
    pub network: ChainId,
    pub amount: TAmount,
    pub pay_to: TAddress,
    pub max_timeout_seconds: u64,
    pub asset: TAddress,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<TExtra>,
}

impl PaymentRequirements {
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn as_concrete<
        'a,
        TScheme: FromStr,
        TAmount: FromStr,
        TAddress: FromStr,
        TExtra: Deserialize<'a>,
    >(
        &'a self,
    ) -> Option<PaymentRequirements<TScheme, TAmount, TAddress, TExtra>> {
        let scheme = self.scheme.parse::<TScheme>().ok()?;
        let amount = self.amount.parse::<TAmount>().ok()?;
        let pay_to = self.pay_to.parse::<TAddress>().ok()?;
        let asset = self.asset.parse::<TAddress>().ok()?;
        let extra = self
            .extra
            .as_ref()
            .and_then(|v| serde_json::from_str::<TExtra>(v.get()).ok());
        Some(PaymentRequirements {
            scheme,
            network: self.network.clone(),
            amount,
            pay_to,
            max_timeout_seconds: self.max_timeout_seconds,
            asset,
            extra,
        })
    }
}

/// Structured representation of a V2 Payment-Required header.
/// This provides proper typing for the payment required response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequired {
    pub x402_version: X402Version2,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub resource: ResourceInfo,
    #[serde(default)]
    pub accepts: Vec<PaymentRequirements>,
}

#[derive(Clone)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct PriceTag {
    pub requirements: PaymentRequirements,
    /// Optional enrichment function provided by concrete price tags
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

/// Type alias for price tag enrichment functions.
/// The function takes a mutable reference to the price tag and the facilitator's
/// supported capabilities, and enriches the price tag (e.g., adds fee_payer for Solana).
pub type Enricher = Arc<dyn Fn(&mut PriceTag, &SupportedResponse) + Send + Sync>;

impl PriceTag {
    /// Apply the stored enrichment function if present.
    #[allow(dead_code)]
    pub fn enrich(&mut self, capabilities: &SupportedResponse) {
        if let Some(enricher) = self.enricher.clone() {
            enricher(self, capabilities);
        }
    }
}
