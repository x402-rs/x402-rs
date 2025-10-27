use crate::network::Network;
use alloy::network::EthereumWallet;
use alloy::signers::local::PrivateKeySigner;
use serde::Deserialize;
use serde::Serialize;
use solana_sdk::signature::Keypair;
use std::env;
use std::str::FromStr;

pub const ENV_SIGNER_TYPE: &str = "SIGNER_TYPE";
pub const ENV_EVM_PRIVATE_KEY: &str = "EVM_PRIVATE_KEY";
pub const ENV_SOLANA_PRIVATE_KEY: &str = "SOLANA_PRIVATE_KEY";

pub const ENV_RPC_BASE: &str = "RPC_URL_BASE";
pub const ENV_RPC_BASE_SEPOLIA: &str = "RPC_URL_BASE_SEPOLIA";
pub const ENV_RPC_XDC: &str = "RPC_URL_XDC";
pub const ENV_RPC_AVALANCHE_FUJI: &str = "RPC_URL_AVALANCHE_FUJI";
pub const ENV_RPC_AVALANCHE: &str = "RPC_URL_AVALANCHE";
pub const ENV_RPC_XRPL_EVM: &str = "RPC_URL_XRPL_EVM";
pub const ENV_RPC_SOLANA: &str = "RPC_URL_SOLANA";
pub const ENV_RPC_SOLANA_DEVNET: &str = "RPC_URL_SOLANA_DEVNET";
pub const ENV_RPC_POLYGON_AMOY: &str = "RPC_URL_POLYGON_AMOY";
pub const ENV_RPC_POLYGON: &str = "RPC_URL_POLYGON";
pub const ENV_RPC_SEI: &str = "RPC_URL_SEI";
pub const ENV_RPC_SEI_TESTNET: &str = "RPC_URL_SEI_TESTNET";

pub fn rpc_env_name_from_network(network: Network) -> &'static str {
    match network {
        Network::BaseSepolia => ENV_RPC_BASE_SEPOLIA,
        Network::Base => ENV_RPC_BASE,
        Network::XdcMainnet => ENV_RPC_XDC,
        Network::AvalancheFuji => ENV_RPC_AVALANCHE_FUJI,
        Network::Avalanche => ENV_RPC_AVALANCHE,
        Network::XrplEvm => ENV_RPC_XRPL_EVM,
        Network::Solana => ENV_RPC_SOLANA,
        Network::SolanaDevnet => ENV_RPC_SOLANA_DEVNET,
        Network::PolygonAmoy => ENV_RPC_POLYGON_AMOY,
        Network::Polygon => ENV_RPC_POLYGON,
        Network::Sei => ENV_RPC_SEI,
        Network::SeiTestnet => ENV_RPC_SEI_TESTNET,
    }
}

/// Supported methods for constructing an Ethereum wallet from environment variables.
#[derive(Debug, Hash, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignerType {
    /// A local private key stored in the `EVM_PRIVATE_KEY` environment variable.
    #[serde(rename = "private-key")]
    PrivateKey,
}

impl SignerType {
    /// Parse the signer type from the `SIGNER_TYPE` environment variable.
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let signer_type_string =
            env::var(ENV_SIGNER_TYPE).map_err(|_| format!("env {ENV_SIGNER_TYPE} not set"))?;
        match signer_type_string.as_str() {
            "private-key" => Ok(SignerType::PrivateKey),
            _ => Err(format!("Unknown signer type {signer_type_string}").into()),
        }
    }

    /// Constructs an [`EthereumWallet`] based on the [`SignerType`] selected from environment.
    ///
    /// Currently only supports [`SignerType::PrivateKey`] variant, based on the following environment variables:
    /// - `SIGNER_TYPE` — currently only `"private-key"` is supported
    /// - `EVM_PRIVATE_KEY` — comma-separated list of private keys used to sign transactions
    pub fn make_evm_wallet(&self) -> Result<EthereumWallet, Box<dyn std::error::Error>> {
        match self {
            SignerType::PrivateKey => {
                let raw_keys = env::var(ENV_EVM_PRIVATE_KEY)
                    .map_err(|_| format!("env {ENV_EVM_PRIVATE_KEY} not set"))?;
                let signers = raw_keys
                    .split(',')
                    .map(str::trim)
                    .filter(|entry| !entry.is_empty())
                    .map(PrivateKeySigner::from_str)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|err| -> Box<dyn std::error::Error> { Box::new(err) })?;
                if signers.is_empty() {
                    return Err("env EVM_PRIVATE_KEY did not contain any private keys".into());
                }

                let mut iter = signers.into_iter();
                let first_signer = iter
                    .next()
                    .expect("iterator contains at least one element by construction");
                let mut wallet = EthereumWallet::from(first_signer);

                for signer in iter {
                    wallet.register_signer(signer);
                }

                Ok(wallet)
            }
        }
    }

    pub fn make_solana_wallet(&self) -> Result<Keypair, Box<dyn std::error::Error>> {
        match self {
            SignerType::PrivateKey => {
                let private_key = env::var(ENV_SOLANA_PRIVATE_KEY)
                    .map_err(|_| format!("env {ENV_SOLANA_PRIVATE_KEY} not set"))?;
                let keypair = Keypair::from_base58_string(private_key.as_str());
                Ok(keypair)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy::network::{Ethereum as AlloyEthereum, NetworkWallet};
    use alloy::signers::local::PrivateKeySigner;
    use std::str::FromStr;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvOverride {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvOverride {
        fn new(key: &'static str) -> Self {
            Self {
                key,
                original: env::var(key).ok(),
            }
        }

        fn set(&self, value: &str) {
            unsafe { env::set_var(self.key, value) };
        }
    }

    impl Drop for EnvOverride {
        fn drop(&mut self) {
            match &self.original {
                Some(value) => unsafe { env::set_var(self.key, value) },
                None => unsafe { env::remove_var(self.key) },
            }
        }
    }

    #[test]
    fn make_evm_wallet_supports_multiple_private_keys() {
        let _guard = ENV_LOCK.lock().expect("env lock poisoned");
        let signer_type_override = EnvOverride::new(ENV_SIGNER_TYPE);
        let evm_keys_override = EnvOverride::new(ENV_EVM_PRIVATE_KEY);

        const KEY_1: &str = "0xcafe000000000000000000000000000000000000000000000000000000000001";
        const KEY_2: &str = "0xcafe000000000000000000000000000000000000000000000000000000000002";

        signer_type_override.set("private-key");
        evm_keys_override.set(&format!("{KEY_1},{KEY_2}"));

        let signer_type = SignerType::from_env().expect("SIGNER_TYPE");
        let wallet = signer_type
            .make_evm_wallet()
            .expect("wallet constructed from env");

        let expected_primary = PrivateKeySigner::from_str(KEY_1)
            .expect("key1 parses")
            .address();
        let expected_secondary = PrivateKeySigner::from_str(KEY_2)
            .expect("key2 parses")
            .address();

        assert_eq!(
            NetworkWallet::<AlloyEthereum>::default_signer_address(&wallet),
            expected_primary
        );

        let signers: Vec<_> = NetworkWallet::<AlloyEthereum>::signer_addresses(&wallet).collect();
        assert_eq!(signers.len(), 2);
        assert!(signers.contains(&expected_primary));
        assert!(signers.contains(&expected_secondary));
    }
}
