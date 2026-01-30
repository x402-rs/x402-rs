use serde::{Deserialize, Deserializer, Serialize, Serializer};
use solana_pubkey::Pubkey;
use std::fmt::{Debug, Display, Formatter};
use std::str::FromStr;
use x402_types::chain::{ChainId, DeployedTokenAmount};
use x402_types::util::money_amount::{MoneyAmount, MoneyAmountParseError};

use crate::networks::KnownNetworkSolana;

/// The CAIP-2 namespace for Solana chains.
pub const SOLANA_NAMESPACE: &str = "solana";

/// A Solana chain reference consisting of 32 ASCII characters.
///
/// The reference is the first 32 characters of the base58-encoded genesis block hash,
/// which uniquely identifies a Solana network. This follows the CAIP-2 standard for
/// Solana chain identification.
///
/// # Well-Known References
///
/// - Mainnet: `5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp`
/// - Devnet: `EtWTRABZaYq6iMfeYKouRu166VU2xqa1`
///
/// # Example
///
/// ```
/// use x402_chain_solana::chain::SolanaChainReference;
/// use x402_chain_solana::KnownNetworkSolana;
///
/// let mainnet = SolanaChainReference::solana();
/// assert_eq!(mainnet.as_str(), "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct SolanaChainReference([u8; 32]);

impl SolanaChainReference {
    /// Creates a new [`SolanaChainReference`] from a 32-byte ASCII array.
    ///
    /// # Panics
    ///
    /// This function does not validate that the bytes are valid ASCII.
    /// Use [`FromStr`] for validated parsing.
    #[allow(dead_code)]
    pub const fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns the underlying bytes.
    #[allow(dead_code)]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Returns the chain reference as a string.
    pub fn as_str(&self) -> &str {
        // Safe because we validate ASCII on construction
        std::str::from_utf8(&self.0).expect("SolanaChainReference contains valid ASCII")
    }
}

impl KnownNetworkSolana<SolanaChainReference> for SolanaChainReference {
    fn solana() -> Self {
        Self::new(*b"5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp")
    }

    fn solana_devnet() -> Self {
        Self::new(*b"EtWTRABZaYq6iMfeYKouRu166VU2xqa1")
    }
}

impl Debug for SolanaChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str("SolanaChainReference(")?;
        f.write_str(self.as_str())?;
        f.write_str(")")
    }
}

impl FromStr for SolanaChainReference {
    type Err = SolanaChainReferenceFormatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if !(s.is_ascii() && s.len() == 32) {
            return Err(SolanaChainReferenceFormatError::InvalidReference(
                s.to_string(),
            ));
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(s.as_bytes());
        Ok(Self(bytes))
    }
}

impl Display for SolanaChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for SolanaChainReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SolanaChainReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl From<SolanaChainReference> for ChainId {
    fn from(value: SolanaChainReference) -> Self {
        ChainId::new(SOLANA_NAMESPACE, value.as_str())
    }
}

impl TryFrom<ChainId> for SolanaChainReference {
    type Error = SolanaChainReferenceFormatError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        if value.namespace != SOLANA_NAMESPACE {
            return Err(SolanaChainReferenceFormatError::InvalidNamespace(
                value.namespace,
            ));
        }
        let solana_chain_reference = Self::from_str(&value.reference)
            .map_err(|_| SolanaChainReferenceFormatError::InvalidReference(value.reference))?;
        Ok(solana_chain_reference)
    }
}

/// Error type for parsing Solana chain references.
#[derive(Debug, thiserror::Error)]
pub enum SolanaChainReferenceFormatError {
    /// The namespace was not "solana".
    #[error("Invalid namespace {0}, expected solana")]
    InvalidNamespace(String),
    /// The reference was not a valid 32-character ASCII string.
    #[error("Invalid solana chain reference {0}")]
    InvalidReference(String),
}

/// Information about an SPL token deployment on a Solana network.
///
/// This type contains all the information needed to interact with a specific
/// token on a specific Solana network, including the mint address and decimal
/// precision.
///
/// # Example
///
/// ```rust
/// use x402_chain_solana::chain::{SolanaChainReference, SolanaTokenDeployment, Address};
/// use x402_chain_solana::KnownNetworkSolana;
/// use std::str::FromStr;
///
/// // USDC on Solana mainnet
/// let usdc = SolanaTokenDeployment::new(
///     SolanaChainReference::solana(),
///     Address::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap(),
///     6,
/// );
///
/// // Parse a human-readable amount
/// let amount = usdc.parse("10.50").unwrap();
/// assert_eq!(amount.amount, 10_500_000); // 10.50 * 10^6
/// ```
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct SolanaTokenDeployment {
    /// The Solana network where this token is deployed.
    pub chain_reference: SolanaChainReference,
    /// The SPL token mint address.
    pub address: Address,
    /// The number of decimal places for this token.
    pub decimals: u8,
}

impl SolanaTokenDeployment {
    /// Creates a new token deployment.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn new(chain_reference: SolanaChainReference, address: Address, decimals: u8) -> Self {
        Self {
            chain_reference,
            address,
            decimals,
        }
    }

    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn amount(&self, v: u64) -> DeployedTokenAmount<u64, SolanaTokenDeployment> {
        DeployedTokenAmount {
            amount: v,
            token: self.clone(),
        }
    }

    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn parse<V>(
        &self,
        v: V,
    ) -> Result<DeployedTokenAmount<u64, SolanaTokenDeployment>, MoneyAmountParseError>
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
        let multiplier = 10u64
            .checked_pow(scale_diff)
            .ok_or(MoneyAmountParseError::OutOfRange)?;
        let digits = u64::try_from(money_amount.mantissa()).expect("mantissa fits in u64");
        let value = digits
            .checked_mul(multiplier)
            .ok_or(MoneyAmountParseError::OutOfRange)?;
        Ok(DeployedTokenAmount {
            amount: value,
            token: self.clone(),
        })
    }
}

/// A Solana public key address.
///
/// This is a wrapper around [`Pubkey`] that provides serialization as a
/// base58-encoded string, suitable for use in x402 protocol messages.
///
/// # Example
///
/// ```
/// use x402_chain_solana::chain::Address;
/// use std::str::FromStr;
///
/// let addr = Address::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v").unwrap();
/// assert_eq!(addr.to_string(), "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
/// ```
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Address(Pubkey);

impl Address {
    /// Creates a new address from a [`Pubkey`].
    pub const fn new(pubkey: Pubkey) -> Self {
        Self(pubkey)
    }

    pub fn pubkey(&self) -> &Pubkey {
        &self.0
    }
}

impl From<Pubkey> for Address {
    fn from(pubkey: Pubkey) -> Self {
        Self(pubkey)
    }
}

impl From<Address> for Pubkey {
    fn from(address: Address) -> Self {
        address.0
    }
}

impl AsRef<[u8]> for Address {
    fn as_ref(&self) -> &[u8] {
        self.0.as_ref()
    }
}

impl Serialize for Address {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let base58_string = self.0.to_string();
        serializer.serialize_str(&base58_string)
    }
}

impl<'de> Deserialize<'de> for Address {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let pubkey = Pubkey::from_str(&s)
            .map_err(|_| serde::de::Error::custom("Failed to decode Solana address"))?;
        Ok(Self(pubkey))
    }
}

impl Display for Address {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Address {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let pubkey =
            Pubkey::from_str(s).map_err(|_| format!("Failed to decode Solana address: {s}"))?;
        Ok(Self(pubkey))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_deployment(decimals: u8) -> SolanaTokenDeployment {
        let chain_ref = SolanaChainReference::solana();
        // Use a well-known test address (USDC on Solana devnet)
        let address = Address::from_str("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZ5nc4pb").unwrap();
        SolanaTokenDeployment::new(chain_ref, address, decimals)
    }

    #[test]
    fn test_parse_whole_number() {
        let deployment = create_test_deployment(6); // 6 decimals like USDC
        let result = deployment.parse("100");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, 100_000_000); // 100 * 10^6
    }

    #[test]
    fn test_parse_with_decimals() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("1.50");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, 1_500_000); // 1.50 * 10^6
    }

    #[test]
    fn test_parse_zero_decimals() {
        let deployment = create_test_deployment(0);
        let result = deployment.parse("42");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, 42);
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
        assert_eq!(result.unwrap().amount, 123_456_789);
    }

    #[test]
    fn test_parse_smallest_amount() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("0.000001");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, 1);
    }

    #[test]
    fn test_parse_with_currency_symbol() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("$10.50");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, 10_500_000);
    }

    #[test]
    fn test_parse_with_commas() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("1,000");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, 1_000_000_000);
    }

    #[test]
    fn test_parse_large_amount() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("999999999");
        assert!(result.is_ok());
        // 999999999 * 10^6 = 999999999000000
        assert_eq!(result.unwrap().amount, 999_999_999_000_000);
    }

    #[test]
    fn test_parse_overflow_returns_error() {
        // Create a deployment with 19 decimals (beyond what u64 can handle)
        let deployment = create_test_deployment(19);
        // 999999999 with 19 decimals = 999999999 * 10^19, which overflows u64
        let result = deployment.parse("999999999");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MoneyAmountParseError::OutOfRange
        ));
    }
}
