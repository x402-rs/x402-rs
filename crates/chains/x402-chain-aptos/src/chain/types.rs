use move_core_types::account_address::AccountAddress;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Debug, Display, Formatter};
use std::str::FromStr;
use x402_types::chain::{ChainId, DeployedTokenAmount};

pub const APTOS_NAMESPACE: &str = "aptos";

/// An Aptos chain reference - the chain ID (1 for mainnet, 2 for testnet)
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct AptosChainReference(u8);

impl AptosChainReference {
    pub fn new(chain_id: u8) -> Self {
        Self(chain_id)
    }

    pub fn chain_id(&self) -> u8 {
        self.0
    }

    pub fn mainnet() -> Self {
        Self(1)
    }

    pub fn testnet() -> Self {
        Self(2)
    }

    /// Alias for mainnet for compatibility with KnownNetworkAptos trait
    pub fn aptos() -> Self {
        Self::mainnet()
    }

    /// Alias for testnet for compatibility with KnownNetworkAptos trait
    pub fn aptos_testnet() -> Self {
        Self::testnet()
    }
}

impl Debug for AptosChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "AptosChainReference({})", self.0)
    }
}

impl Display for AptosChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for AptosChainReference {
    type Err = AptosChainReferenceFormatError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let chain_id = s
            .parse::<u8>()
            .map_err(|_| AptosChainReferenceFormatError::InvalidReference(s.to_string()))?;
        if chain_id != 1 && chain_id != 2 {
            return Err(AptosChainReferenceFormatError::InvalidReference(format!(
                "Invalid Aptos chain ID: {}",
                chain_id
            )));
        }
        Ok(Self(chain_id))
    }
}

impl Serialize for AptosChainReference {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for AptosChainReference {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

impl From<AptosChainReference> for ChainId {
    fn from(value: AptosChainReference) -> Self {
        ChainId::new(APTOS_NAMESPACE, value.0.to_string())
    }
}

impl TryFrom<ChainId> for AptosChainReference {
    type Error = AptosChainReferenceFormatError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        if value.namespace != APTOS_NAMESPACE {
            return Err(AptosChainReferenceFormatError::InvalidNamespace(
                value.namespace,
            ));
        }
        Self::from_str(&value.reference)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum AptosChainReferenceFormatError {
    #[error("Invalid namespace {0}, expected aptos")]
    InvalidNamespace(String),
    #[error("Invalid aptos chain reference {0}")]
    InvalidReference(String),
}

/// Aptos address type
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct Address(AccountAddress);

impl Address {
    pub fn new(address: AccountAddress) -> Self {
        Self(address)
    }

    pub fn inner(&self) -> &AccountAddress {
        &self.0
    }
}

impl From<AccountAddress> for Address {
    fn from(address: AccountAddress) -> Self {
        Self(address)
    }
}

impl From<Address> for AccountAddress {
    fn from(address: Address) -> Self {
        address.0
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
        let address =
            AccountAddress::from_str(s).map_err(|e| format!("Invalid Aptos address: {}", e))?;
        Ok(Self(address))
    }
}

impl Serialize for Address {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_hex_literal())
    }
}

impl<'de> Deserialize<'de> for Address {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Token deployment information for Aptos.
///
/// Contains the chain reference, token address, and decimals for a token deployed
/// on an Aptos network.
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct AptosTokenDeployment {
    /// The Aptos network where this token is deployed.
    pub chain_reference: AptosChainReference,
    /// The fungible asset address.
    pub address: Address,
    /// The number of decimal places for this token.
    pub decimals: u8,
}

impl AptosTokenDeployment {
    /// Creates a new token deployment.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn new(chain_reference: AptosChainReference, address: Address, decimals: u8) -> Self {
        Self {
            chain_reference,
            address,
            decimals,
        }
    }

    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn amount(&self, v: u64) -> DeployedTokenAmount<u64, AptosTokenDeployment> {
        DeployedTokenAmount {
            amount: v,
            token: self.clone(),
        }
    }
}
