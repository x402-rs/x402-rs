use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fmt::Display;

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

impl Into<crate::proto::SettleResponse> for SettleResponse {
    fn into(self) -> crate::proto::SettleResponse {
        crate::proto::SettleResponse(
            serde_json::to_value(self).expect("SettleResponse serialization failed"),
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
