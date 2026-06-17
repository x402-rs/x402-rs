//! TRON chain type definitions.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Debug, Display, Formatter};
use std::str::FromStr;
use x402_types::chain::ChainId;

use crate::chain::TronAddress;

/// The CAIP-2 namespace for TRON chains.
pub const TRON_NAMESPACE: &str = "tron";

/// A TRON chain reference: the last 4 bytes of the genesis block hash (TIP-474).
///
/// Stored as a `u32` and serialized as a lowercase `0x`-prefixed 8-digit hex string
/// (e.g. `"0x2b6653dc"`) to match the CAIP-2 specification for the `tron` namespace.
///
/// Well-known references are provided via [`crate::KnownNetworkTron`]:
///
/// | Network | Reference      | Chain ID   |
/// |---------|----------------|------------|
/// | Mainnet | `0x2b6653dc`   | 728126428  |
/// | Shasta  | `0xcd8690dc`   | 3448148188 |
/// | Nile    | `0x94a9059e`   | 2494104990 |
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct TronChainReference(u32);

pub const TRON_MAINNET: TronChainReference = TronChainReference(0x2b6653dc);
pub const TRON_SHASTA: TronChainReference = TronChainReference(0xcd8690dc);
pub const TRON_NILE: TronChainReference = TronChainReference(0x94a9059e);

impl TronChainReference {
    /// Creates a new chain reference from a raw u32 chain ID.
    pub const fn new(chain_id: u32) -> Self {
        Self(chain_id)
    }

    /// Returns the numeric chain ID value.
    pub fn inner(self) -> u32 {
        self.0
    }

    /// Returns the CAIP-2 chain ID (e.g. `tron:0x2b6653dc`).
    pub fn chain_id(self) -> ChainId {
        ChainId::new(TRON_NAMESPACE, self.to_string())
    }

    /// Returns the SUN.io Permit2 contract address for this network.
    ///
    /// This is the `verifyingContract` in the EIP-712 domain that clients sign against.
    /// It is NOT the x402ExactPermit2Proxy — that is the `spender` in the Permit2 message.
    pub fn sun_permit2(self) -> Option<TronAddress> {
        match self {
            TRON_MAINNET => "TTJxU3P8rHycAyFY4kVtGNfmnMH4ezcuM9".parse().ok(),
            TRON_SHASTA => "TCJjTtzwRJYPapGTdyJdKcr7MqkngRRWQx".parse().ok(),
            TRON_NILE => "TCJjTtzwRJYPapGTdyJdKcr7MqkngRRWQx".parse().ok(),
            _ => None,
        }
    }

    /// Returns the x402ExactPermit2Proxy address for this network.
    ///
    /// This is the `spender` in the Permit2 message and the contract the facilitator
    /// calls to settle. It is NOT the SUN.io Permit2 verifying contract.
    pub fn x402_exact_permit2_proxy(self) -> Option<TronAddress> {
        match self {
            TRON_NILE => "TTjbkCh8sC4gNTWG48iWNssrLBqZq2tiTy".parse().ok(),
            _ => None,
        }
    }
}

impl Debug for TronChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "TronChainReference({})", self)
    }
}

impl Display for TronChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "0x{:08x}", self.0)
    }
}

impl FromStr for TronChainReference {
    type Err = TronChainReferenceFormatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let hex = s
            .strip_prefix("0x")
            .or_else(|| s.strip_prefix("0X"))
            .ok_or_else(|| TronChainReferenceFormatError::InvalidReference(s.to_string()))?;
        let v = u32::from_str_radix(hex, 16)
            .map_err(|_| TronChainReferenceFormatError::InvalidReference(s.to_string()))?;
        Ok(Self(v))
    }
}

impl Serialize for TronChainReference {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for TronChainReference {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl From<TronChainReference> for ChainId {
    fn from(value: TronChainReference) -> Self {
        ChainId::new(TRON_NAMESPACE, value.to_string())
    }
}

impl From<&TronChainReference> for ChainId {
    fn from(value: &TronChainReference) -> Self {
        ChainId::new(TRON_NAMESPACE, value.to_string())
    }
}

// For Alloy chain id
impl From<&TronChainReference> for u64 {
    fn from(value: &TronChainReference) -> Self {
        value.0 as u64
    }
}

impl TryFrom<ChainId> for TronChainReference {
    type Error = TronChainReferenceFormatError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        Self::try_from(&value)
    }
}

impl TryFrom<&ChainId> for TronChainReference {
    type Error = TronChainReferenceFormatError;

    fn try_from(value: &ChainId) -> Result<Self, Self::Error> {
        if value.namespace != TRON_NAMESPACE {
            return Err(TronChainReferenceFormatError::InvalidNamespace(
                value.namespace.to_string(),
            ));
        }
        value.reference.parse().map_err(|_| {
            TronChainReferenceFormatError::InvalidReference(value.reference.to_string())
        })
    }
}

/// Error returned when converting a [`ChainId`] to a [`TronChainReference`].
#[derive(Debug, thiserror::Error)]
pub enum TronChainReferenceFormatError {
    #[error("Invalid namespace {0:?}, expected \"tron\"")]
    InvalidNamespace(String),
    #[error("Invalid TRON chain reference {0:?}; expected 0x-prefixed hex (e.g. \"0x2b6653dc\")")]
    InvalidReference(String),
}

/// Asset transfer method for a TRON token deployment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "assetTransferMethod")]
pub enum TronTransferMethod {
    /// EIP-3009 `transferWithAuthorization` (TIP-712 domain).
    #[serde(rename = "eip3009")]
    Eip3009 {
        /// Token name for the EIP-712 domain.
        name: String,
        /// Token version for the EIP-712 domain.
        version: String,
    },
    /// Permit2 transfer method.
    #[serde(rename = "permit2")]
    Permit2 {
        /// The token name as specified in the EIP-712 domain.
        name: String,
        /// The token version as specified in the EIP-712 domain.
        version: String,
    },
}

/// Information about a token deployment on a TRON network.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TronTokenDeployment {
    /// The TRON network this deployment is on.
    pub chain_reference: TronChainReference,
    /// The token contract address in Base58Check format.
    pub address: String,
    /// Number of decimal places (e.g., 6 for USDC/USDT).
    pub decimals: u8,
    /// The method used to transfer the asset.
    pub transfer_method: TronTransferMethod,
}
