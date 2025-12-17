use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fmt::Display;
use crate::proto;

/// Version 1 of the x402 protocol.
#[derive(Debug, Copy, Clone, Default, PartialEq, Eq)]
pub struct X402Version1;

impl X402Version1 {
    pub const VALUE: u8 = 1;
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

pub enum SettleResponse {
    Success {
        payer: String,
        transaction: String,
        network: String,
    },
    Error {
        reason: String,
        network: String,
    },
}

impl From<SettleResponse> for crate::proto::SettleResponse {
    fn from(val: SettleResponse) -> Self {
        crate::proto::SettleResponse(
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

impl From<VerifyResponse> for crate::proto::VerifyResponse {
    fn from(val: VerifyResponse) -> Self {
        crate::proto::VerifyResponse(
            serde_json::to_value(val).expect("VerifyResponse serialization failed"),
        )
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyRequest<TPayload, TRequirements> {
    pub x402_version: X402Version1,
    pub payment_payload: TPayload,
    pub payment_requirements: TRequirements,
}