//! BCH chain identifiers used by the x402 BCH adapter.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Display, Formatter};
use std::str::FromStr;
use x402_types::chain::ChainId;

/// The x402/WalletConnect namespace for Bitcoin Cash networks.
pub const BCH_NAMESPACE: &str = "bch";

/// A Bitcoin Cash network reference.
///
/// These references intentionally follow the BCH WalletConnect vocabulary.
/// They are not the `bip122:<genesis-hash>` form; BCH uses its CashAddr
/// network prefixes as the chain references.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BchChainReference {
    Mainnet,
    Chipnet,
}

impl BchChainReference {
    /// The BCH mainnet identifier used on the wire.
    pub const MAINNET: Self = Self::Mainnet;
    /// The BCH Chipnet identifier used on the wire.
    pub const CHIPNET: Self = Self::Chipnet;

    /// Returns the x402 `ChainId` for this BCH network.
    pub fn chain_id(self) -> ChainId {
        ChainId::new(BCH_NAMESPACE, self.as_str())
    }

    /// Returns the reference component without the namespace.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Mainnet => "bitcoincash",
            Self::Chipnet => "bchtest",
        }
    }

    /// Returns whether this is a network intended for testing only.
    pub const fn is_test_network(self) -> bool {
        !matches!(self, Self::Mainnet)
    }
}

impl Display for BchChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for BchChainReference {
    type Err = BchChainReferenceFormatError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "bitcoincash" => Ok(Self::Mainnet),
            "bchtest" => Ok(Self::Chipnet),
            other => Err(BchChainReferenceFormatError::InvalidReference(
                other.to_owned(),
            )),
        }
    }
}

impl Serialize for BchChainReference {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for BchChainReference {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        value.parse().map_err(serde::de::Error::custom)
    }
}

impl From<BchChainReference> for ChainId {
    fn from(value: BchChainReference) -> Self {
        value.chain_id()
    }
}

impl TryFrom<ChainId> for BchChainReference {
    type Error = BchChainReferenceFormatError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        if value.namespace != BCH_NAMESPACE {
            return Err(BchChainReferenceFormatError::InvalidNamespace(
                value.namespace,
            ));
        }
        value.reference.parse()
    }
}

/// Error returned when a generic x402 chain ID is not a supported BCH ID.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BchChainReferenceFormatError {
    #[error("invalid BCH namespace {0:?}, expected \"{BCH_NAMESPACE}\"")]
    InvalidNamespace(String),
    #[error("invalid BCH network reference {0:?}; expected bitcoincash or bchtest")]
    InvalidReference(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emits_walletconnect_compatible_network_ids() {
        assert_eq!(
            BchChainReference::MAINNET.chain_id().to_string(),
            "bch:bitcoincash"
        );
        assert_eq!(
            BchChainReference::CHIPNET.chain_id().to_string(),
            "bch:bchtest"
        );
    }

    #[test]
    fn generic_chain_ids_round_trip_through_typed_bch_identity() {
        for network in [BchChainReference::MAINNET, BchChainReference::CHIPNET] {
            let chain_id = network.chain_id();
            assert_eq!(BchChainReference::try_from(chain_id).unwrap(), network);
        }
    }

    #[test]
    fn rejects_other_namespaces_and_genesis_hash_references() {
        assert!(matches!(
            BchChainReference::try_from(ChainId::new(
                "bip122",
                "000000000019d6689c085ae165831e93"
            )),
            Err(BchChainReferenceFormatError::InvalidNamespace(namespace)) if namespace == "bip122"
        ));
        assert!(matches!(
            BchChainReference::try_from(ChainId::new("bch", "mainnet")),
            Err(BchChainReferenceFormatError::InvalidReference(reference)) if reference == "mainnet"
        ));
        assert!(matches!(
            BchChainReference::try_from(ChainId::new("bch", "chipnet")),
            Err(BchChainReferenceFormatError::InvalidReference(reference)) if reference == "chipnet"
        ));
    }

    #[test]
    fn serializes_the_reference_not_the_full_chain_id() {
        assert_eq!(
            serde_json::to_string(&BchChainReference::MAINNET).unwrap(),
            "\"bitcoincash\""
        );
        assert_eq!(
            serde_json::from_str::<BchChainReference>("\"bchtest\"").unwrap(),
            BchChainReference::CHIPNET
        );
    }
}
