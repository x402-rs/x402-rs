use crate::network::Network;
use alloy::network::EthereumWallet;
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller,
};
use alloy::providers::{Identity, ProviderBuilder, RootProvider};
use alloy::signers::local::PrivateKeySigner;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::env;
use std::sync::Arc;

pub type EthereumProvider = FillProvider<
    JoinFill<
        JoinFill<
            Identity,
            JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
        >,
        WalletFiller<EthereumWallet>,
    >,
    RootProvider,
>;

pub struct ProviderCache {
    providers: HashMap<Network, Arc<EthereumProvider>>,
}

impl ProviderCache {
    pub async fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let mut providers = HashMap::new();
        let signer_type = SignerType::from_env()?;
        let wallet = make_wallet(&signer_type)?;
        tracing::info!("Using address: {}", wallet.default_signer().address());

        for network in Network::variants() {
            let env_var = match network {
                Network::BaseSepolia => "RPC_URL_BASE_SEPOLIA",
                Network::Base => "RPC_URL_BASE",
            };

            let rpc_url = env::var(env_var);
            if let Ok(rpc_url) = rpc_url {
                let provider = ProviderBuilder::new()
                    .wallet(wallet.clone())
                    .connect(&rpc_url)
                    .await
                    .map_err(|e| format!("Failed to connect to {}: {}", network, e))?;
                providers.insert(*network, Arc::new(provider));
                tracing::info!("Initialized provider for {} at {}", network, rpc_url);
            }
        }

        Ok(Self { providers })
    }

    pub fn by_network(&self, network: Network) -> Option<&Arc<EthereumProvider>> {
        self.providers.get(&network)
    }
}

fn make_wallet(signer_type: &SignerType) -> Result<EthereumWallet, Box<dyn std::error::Error>> {
    match signer_type {
        SignerType::PrivateKey => {
            let private_key = env::var("PRIVATE_KEY").map_err(|_| "env PRIVATE_KEY not set")?;
            let pk_signer: PrivateKeySigner = private_key.parse()?;
            Ok(EthereumWallet::new(pk_signer))
        }
    }
}

#[derive(Debug, Hash, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum SignerType {
    #[serde(rename = "private-key")]
    PrivateKey,
}

impl SignerType {
    fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let signer_type_string = env::var("SIGNER_TYPE").map_err(|_| "env SIGNER_TYPE not set")?;
        match signer_type_string.as_str() {
            "private-key" => Ok(SignerType::PrivateKey),
            _ => Err(format!("Unknown signer type {}", signer_type_string).into()),
        }
    }
}
