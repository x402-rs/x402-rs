use aptos_crypto::ed25519::Ed25519PrivateKey;
use aptos_rest_client::Client as AptosClient;
use move_core_types::account_address::AccountAddress;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use x402_types::chain::{ChainId, ChainProviderOps};
use x402_types::scheme::X402SchemeFacilitatorError;

use crate::chain::config::AptosChainConfig;
use crate::chain::types::{Address, AptosChainReference};

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
