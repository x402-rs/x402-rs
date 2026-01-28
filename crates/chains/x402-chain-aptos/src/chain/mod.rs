pub mod config;

use aptos_crypto::ed25519::Ed25519PrivateKey;
use aptos_rest_client::Client as AptosClient;
use aptos_types::account_address::AccountAddress;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt::{Debug, Display, Formatter};
use std::str::FromStr;
use std::sync::Arc;
use x402_types::chain::{ChainId, ChainProviderOps, DeployedTokenAmount};
use x402_types::scheme::X402SchemeFacilitatorError;

use crate::chain::config::AptosChainConfig;

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

#[derive(thiserror::Error, Debug)]
#[allow(clippy::enum_variant_names)]
pub enum AptosChainProviderError {
    #[error("BCS deserialization error: {0}")]
    BcsError(#[from] bcs::Error),
    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

impl From<AptosChainProviderError> for X402SchemeFacilitatorError {
    fn from(value: AptosChainProviderError) -> Self {
        Self::OnchainFailure(value.to_string())
    }
}

pub struct AptosChainProvider {
    chain: AptosChainReference,
    sponsor_gas: bool,
    fee_payer_address: Option<AccountAddress>,
    fee_payer_private_key: Option<Ed25519PrivateKey>,
    rest_client: Arc<AptosClient>,
}

impl Debug for AptosChainProvider {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AptosChainProvider")
            .field("chain", &self.chain)
            .field("sponsor_gas", &self.sponsor_gas)
            .field("rpc_url", &"<rest_client>")
            .finish()
    }
}

impl AptosChainProvider {
    pub async fn from_config(
        config: &AptosChainConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let chain = config.chain_reference();
        let rpc_url = config.rpc();
        let sponsor_gas = config.sponsor_gas();

        // Validate: if sponsoring, signer must be provided
        if sponsor_gas && config.signer().is_none() {
            return Err("signer configuration required when sponsor_gas is true".into());
        }

        // Parse private key if signer is provided
        let (fee_payer_address, fee_payer_private_key) = if let Some(signer) = config.signer() {
            let private_key_hex = signer.to_string();
            let private_key_hex = private_key_hex.trim_start_matches("0x");
            let private_key_bytes = hex::decode(private_key_hex)?;
            let private_key = Ed25519PrivateKey::try_from(private_key_bytes.as_slice())?;

            // Derive account address from public key
            use aptos_crypto::ed25519::Ed25519PublicKey;
            use aptos_types::transaction::authenticator::AuthenticationKey;

            let public_key: Ed25519PublicKey = (&private_key).into();
            let auth_key = AuthenticationKey::ed25519(&public_key);
            let account_address = auth_key.account_address();

            (Some(account_address), Some(private_key))
        } else {
            (None, None)
        };

        // Create REST client with optional API key
        let rest_client = if let Some(api_key) = config.api_key() {
            use aptos_rest_client::AptosBaseUrl;
            AptosClient::builder(AptosBaseUrl::Custom(rpc_url.clone()))
                .api_key(api_key)?
                .build()
        } else {
            AptosClient::new(rpc_url.clone())
        };

        let provider = Self::new(
            chain,
            sponsor_gas,
            fee_payer_address,
            fee_payer_private_key,
            rest_client,
        );
        Ok(provider)
    }

    pub fn new(
        chain: AptosChainReference,
        sponsor_gas: bool,
        fee_payer_address: Option<AccountAddress>,
        fee_payer_private_key: Option<Ed25519PrivateKey>,
        rest_client: AptosClient,
    ) -> Self {
        #[cfg(feature = "telemetry")]
        {
            let chain_id: ChainId = chain.into();
            if let Some(address) = fee_payer_address {
                tracing::info!(
                    chain = %chain_id,
                    address = %address,
                    sponsor_gas = sponsor_gas,
                    "Initialized Aptos provider with fee payer"
                );
            } else {
                tracing::info!(
                    chain = %chain_id,
                    sponsor_gas = sponsor_gas,
                    "Initialized Aptos provider without fee payer"
                );
            }
        }
        Self {
            chain,
            sponsor_gas,
            fee_payer_address,
            fee_payer_private_key,
            rest_client: Arc::new(rest_client),
        }
    }

    pub fn rest_client(&self) -> &AptosClient {
        &self.rest_client
    }

    pub fn sponsor_gas(&self) -> bool {
        self.sponsor_gas
    }

    pub fn account_address(&self) -> Option<AccountAddress> {
        self.fee_payer_address
    }

    pub fn private_key(&self) -> Option<&Ed25519PrivateKey> {
        self.fee_payer_private_key.as_ref()
    }
}

impl ChainProviderOps for AptosChainProvider {
    fn signer_addresses(&self) -> Vec<String> {
        if let Some(address) = self.fee_payer_address {
            vec![Address::new(address).to_string()]
        } else {
            vec![]
        }
    }

    fn chain_id(&self) -> ChainId {
        self.chain.into()
    }
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
