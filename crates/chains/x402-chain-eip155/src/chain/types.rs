//! Wire format types for EVM chain interactions.
//!
//! This module provides types that handle serialization and deserialization
//! of EVM-specific values in the x402 protocol wire format.

use alloy_primitives::{Address, U256, hex};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::ops::Mul;
use std::str::FromStr;
use x402_types::chain::{ChainId, DeployedTokenAmount};
use x402_types::util::money_amount::{MoneyAmount, MoneyAmountParseError};

/// An Ethereum address that serializes with EIP-55 checksum encoding.
///
/// This wrapper ensures addresses are always serialized in checksummed format
/// (e.g., `0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045`) for compatibility
/// with the x402 protocol wire format.
///
/// # Example
///
/// ```
/// use x402_chain_eip155::chain::ChecksummedAddress;
///
/// let addr: ChecksummedAddress = "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045".parse().unwrap();
/// assert_eq!(addr.to_string(), "0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045");
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ChecksummedAddress(pub Address);

impl FromStr for ChecksummedAddress {
    type Err = hex::FromHexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let address = Address::from_str(s)?;
        Ok(Self(address))
    }
}

impl Display for ChecksummedAddress {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0.to_checksum(None))
    }
}

impl Serialize for ChecksummedAddress {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_checksum(None))
    }
}

impl<'de> Deserialize<'de> for ChecksummedAddress {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl From<ChecksummedAddress> for Address {
    fn from(value: ChecksummedAddress) -> Self {
        value.0
    }
}

impl From<Address> for ChecksummedAddress {
    fn from(address: Address) -> Self {
        Self(address)
    }
}

impl PartialEq<ChecksummedAddress> for Address {
    fn eq(&self, other: &ChecksummedAddress) -> bool {
        self.eq(&other.0)
    }
}

pub mod decimal_u256 {
    use alloy_primitives::U256;
    use serde::{Deserialize, Deserializer, Serializer};

    /// Serialize a U256 as a decimal string.
    pub fn serialize<S>(value: &U256, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    /// Deserialize a decimal string into a U256.
    pub fn deserialize<'de, D>(deserializer: D) -> Result<U256, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        U256::from_str_radix(&s, 10).map_err(serde::de::Error::custom)
    }
}

/// The CAIP-2 namespace for EVM-compatible chains.
pub const EIP155_NAMESPACE: &str = "eip155";

/// A numeric chain ID for EVM-compatible networks.
///
/// This type wraps the numeric chain ID used by EVM networks (e.g., `1` for Ethereum mainnet,
/// `8453` for Base). It can be converted to/from a [`ChainId`] for use with the x402 protocol.
///
/// # Example
///
/// ```
/// use x402_chain_eip155::chain::Eip155ChainReference;
/// use x402_types::chain::ChainId;
///
/// let base = Eip155ChainReference::new(8453);
/// let chain_id: ChainId = base.into();
/// assert_eq!(chain_id.to_string(), "eip155:8453");
/// ```
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct Eip155ChainReference(u64);

impl Eip155ChainReference {
    /// Converts this chain reference to a CAIP-2 [`ChainId`].
    pub fn as_chain_id(&self) -> ChainId {
        ChainId::new(EIP155_NAMESPACE, self.0.to_string())
    }
}

impl From<Eip155ChainReference> for ChainId {
    fn from(value: Eip155ChainReference) -> Self {
        ChainId::new(EIP155_NAMESPACE, value.0.to_string())
    }
}

impl From<&Eip155ChainReference> for ChainId {
    fn from(value: &Eip155ChainReference) -> Self {
        ChainId::new(EIP155_NAMESPACE, value.0.to_string())
    }
}

impl TryFrom<ChainId> for Eip155ChainReference {
    type Error = Eip155ChainReferenceFormatError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        if value.namespace != EIP155_NAMESPACE {
            return Err(Eip155ChainReferenceFormatError::InvalidNamespace(
                value.namespace,
            ));
        }
        let chain_id: u64 = value.reference.parse().map_err(|_| {
            Eip155ChainReferenceFormatError::InvalidReference(value.reference.clone())
        })?;
        Ok(Eip155ChainReference(chain_id))
    }
}

impl TryFrom<&ChainId> for Eip155ChainReference {
    type Error = Eip155ChainReferenceFormatError;

    fn try_from(value: &ChainId) -> Result<Self, Self::Error> {
        if value.namespace != EIP155_NAMESPACE {
            return Err(Eip155ChainReferenceFormatError::InvalidNamespace(
                value.namespace.clone(),
            ));
        }
        let chain_id: u64 = value.reference.parse().map_err(|_| {
            Eip155ChainReferenceFormatError::InvalidReference(value.reference.clone())
        })?;
        Ok(Eip155ChainReference(chain_id))
    }
}

/// Error returned when converting a [`ChainId`] to an [`Eip155ChainReference`].
#[derive(Debug, thiserror::Error)]
pub enum Eip155ChainReferenceFormatError {
    /// The chain ID namespace is not `eip155`.
    #[error("Invalid namespace {0}, expected eip155")]
    InvalidNamespace(String),
    /// The chain reference is not a valid numeric value.
    #[error("Invalid eip155 chain reference {0}")]
    InvalidReference(String),
}

impl Eip155ChainReference {
    /// Creates a new chain reference from a numeric chain ID.
    pub fn new(chain_id: u64) -> Self {
        Self(chain_id)
    }

    /// Returns the numeric chain ID.
    pub fn inner(&self) -> u64 {
        self.0
    }
}

impl Display for Eip155ChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Information about a token deployment on an EVM chain.
///
/// This type contains all the information needed to interact with a token contract,
/// including its address, decimal places, and optional EIP-712 domain parameters
/// for signature verification.
///
/// # Example
///
/// ```ignore
/// use x402_rs::networks::{KnownNetworkEip155, USDC};
///
/// // Get USDC deployment on Base
/// let usdc = USDC::base();
/// assert_eq!(usdc.decimals, 6);
///
/// // Parse a human-readable amount to token units
/// let amount = usdc.parse("10.50").unwrap();
/// assert_eq!(amount.amount, U256::from(10_500_000u64));
/// ```
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct Eip155TokenDeployment {
    /// The chain this token is deployed on.
    pub chain_reference: Eip155ChainReference,
    /// The token contract address.
    pub address: Address,
    /// Number of decimal places for the token (e.g., 6 for USDC, 18 for most ERC-20s).
    pub decimals: u8,
    /// The method used to transfer assets.
    pub transfer_method: AssetTransferMethod,
}

#[derive(Debug, Clone, Eq, PartialEq, Hash, Serialize)]
#[serde(tag = "assetTransferMethod")]
pub enum AssetTransferMethod {
    /// EIP-712 domain parameters for signature verification of EIP3009 transfers.
    #[serde(rename = "eip3009")]
    Eip3009 {
        /// The token name as specified in the EIP-712 domain.
        name: String,
        /// The token version as specified in the EIP-712 domain.
        version: String,
    },
    /// Permit2 transfer method.
    #[serde(rename = "permit2")]
    Permit2,
}

impl<'de> Deserialize<'de> for AssetTransferMethod {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // --- Wire types (private) ---

        #[derive(Debug, Deserialize)]
        #[serde(untagged)]
        #[allow(dead_code)]
        enum AssetTransferMethodWire {
            // { "assetTransferMethod": "permit2" }
            Permit2Tagged {
                #[serde(rename = "assetTransferMethod")]
                asset_transfer_method: Permit2Tag,
            },
            // { "assetTransferMethod": "eip3009", "name": "...", "version": "..." }
            Eip3009Tagged {
                #[serde(rename = "assetTransferMethod")]
                asset_transfer_method: Eip3009Tag,
                name: String,
                version: String,
            },
            // { "name": "...", "version": "..." }  (implicit)
            Eip3009Implicit {
                name: String,
                version: String,
            },
        }

        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "lowercase")]
        enum Permit2Tag {
            Permit2,
        }

        #[derive(Debug, Deserialize)]
        #[serde(rename_all = "lowercase")]
        enum Eip3009Tag {
            Eip3009,
        }

        let wire = AssetTransferMethodWire::deserialize(deserializer)
            .map_err(|e| serde::de::Error::custom(format!("invalid asset transfer method: {e}")))?;

        Ok(match wire {
            AssetTransferMethodWire::Permit2Tagged { .. } => AssetTransferMethod::Permit2,

            AssetTransferMethodWire::Eip3009Tagged { name, version, .. }
            | AssetTransferMethodWire::Eip3009Implicit { name, version } => {
                AssetTransferMethod::Eip3009 { name, version }
            }
        })
    }
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl Eip155TokenDeployment {
    /// Creates a token amount from a raw value.
    ///
    /// The value should already be in the token's smallest unit (e.g., wei).
    pub fn amount<V: Into<u64>>(&self, v: V) -> DeployedTokenAmount<U256, Eip155TokenDeployment> {
        DeployedTokenAmount {
            amount: U256::from(v.into()),
            token: self.clone(),
        }
    }

    /// Parses a human-readable amount string into token units.
    ///
    /// Accepts formats like `"10.50"`, `"$10.50"`, `"1,000"`, etc.
    /// The amount is scaled by the token's decimal places.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The input cannot be parsed as a number
    /// - The input has more decimal places than the token supports
    /// - The value is out of range
    ///
    /// # Example
    ///
    /// ```ignore
    /// use x402_rs::networks::{KnownNetworkEip155, USDC};
    ///
    /// let usdc = USDC::base();
    /// let amount = usdc.parse("10.50").unwrap();
    /// // 10.50 USDC = 10,500,000 units (6 decimals)
    /// assert_eq!(amount.amount, U256::from(10_500_000u64));
    /// ```
    pub fn parse<V>(
        &self,
        v: V,
    ) -> Result<DeployedTokenAmount<U256, Eip155TokenDeployment>, MoneyAmountParseError>
    where
        V: TryInto<MoneyAmount>,
        MoneyAmountParseError: From<<V as TryInto<MoneyAmount>>::Error>,
    {
        let money_amount = v.try_into()?;
        let scale = money_amount.scale();
        let token_scale = self.decimals as u32;
        if scale > token_scale {
            return Err(MoneyAmountParseError::WrongPrecision {
                money: scale,
                token: token_scale,
            });
        }
        let scale_diff = token_scale - scale;
        let multiplier = U256::from(10).pow(U256::from(scale_diff));
        let digits = money_amount.mantissa();
        let value = U256::from(digits).mul(multiplier);
        Ok(DeployedTokenAmount {
            amount: value,
            token: self.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_deployment(decimals: u8) -> Eip155TokenDeployment {
        let chain_ref = Eip155ChainReference::new(1); // Mainnet
        Eip155TokenDeployment {
            chain_reference: chain_ref,
            address: Address::ZERO,
            decimals,
            transfer_method: AssetTransferMethod::Eip3009 {
                name: "TestToken".into(),
                version: "2".into(),
            },
        }
    }

    #[test]
    fn test_parse_whole_number() {
        let deployment = create_test_deployment(6); // 6 decimals like USDC
        let result = deployment.parse("100");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, U256::from(100_000_000u64)); // 100 * 10^6
    }

    #[test]
    fn test_parse_with_decimals() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("1.50");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, U256::from(1_500_000u64)); // 1.50 * 10^6
    }

    #[test]
    fn test_parse_zero_decimals() {
        let deployment = create_test_deployment(0);
        let result = deployment.parse("42");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, U256::from(42u64));
    }

    #[test]
    fn test_parse_precision_too_high() {
        let deployment = create_test_deployment(2); // Only 2 decimals
        let result = deployment.parse("1.234"); // 3 decimals - should fail
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, MoneyAmountParseError::WrongPrecision { .. }));
    }

    #[test]
    fn test_parse_exact_precision() {
        let deployment = create_test_deployment(9); // 9 decimals
        let result = deployment.parse("0.123456789");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, U256::from(123_456_789u64));
    }

    #[test]
    fn test_parse_smallest_amount() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("0.000001");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, U256::from(1u64));
    }

    #[test]
    fn test_parse_with_currency_symbol() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("$10.50");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, U256::from(10_500_000u64));
    }

    #[test]
    fn test_parse_with_commas() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("1,000");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, U256::from(1_000_000_000u64));
    }

    #[test]
    fn test_parse_large_amount() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("999999999");
        assert!(result.is_ok());
        // 999999999 * 10^6 = 999999999000000
        assert_eq!(result.unwrap().amount, U256::from(999_999_999_000_000u64));
    }

    #[test]
    fn test_parse_very_large_amount_with_high_decimals() {
        // EIP155 uses U256, so we can handle much larger amounts than Solana
        let deployment = create_test_deployment(18); // 18 decimals like ETH
        let result = deployment.parse("999999999"); // 9 digits, 0 decimals
        assert!(result.is_ok());
        // 999999999 * 10^18 = 999999999000000000000000000
        let expected = U256::from(999_999_999u64) * U256::from(10).pow(U256::from(18));
        assert_eq!(result.unwrap().amount, expected);
    }
}
