//! Configuration module for the x402 facilitator server.
//!
//! This module handles configuration loading from JSON files and environment variables.
//! It supports multiple blockchain families (EVM, Solana, Aptos) through a unified
//! configuration interface.
//!
//! # Configuration File Format
//!
//! Configuration is loaded from a JSON file (default: `config.json`) with the following structure:
//!
//! ```json
//! {
//!   "port": 8080,
//!   "host": "0.0.0.0",
//!   "chains": {
//!     "eip155:84532": {
//!       "rpc_url": "https://sepolia.base.org",
//!       "signer_private_key": "0x..."
//!     },
//!     "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp": {
//!       "rpc_url": "https://api.devnet.solana.com",
//!       "signer_private_key": "base58..."
//!     }
//!   },
//!   "schemes": [
//!     {
//!       "version": "v2",
//!       "scheme": "exact",
//!       "network": "eip155:*"
//!     }
//!   ]
//! }
//! ```
//!
//! # Environment Variables
//!
//! - `CONFIG` - Path to configuration file (default: `config.json`)
//! - `PORT` - Server port (default: 8080)
//! - `HOST` - Server bind address (default: `0.0.0.0`)

use serde::{Deserialize, Serialize};
use std::ops::Deref;
use x402_types::chain::ChainId;

#[cfg(feature = "chain-aptos")]
use x402_chain_aptos::chain as aptos;
#[cfg(feature = "chain-aptos")]
use x402_chain_aptos::chain::config::{AptosChainConfig, AptosChainConfigInner};
#[cfg(feature = "chain-eip155")]
use x402_chain_eip155::chain as eip155;
#[cfg(feature = "chain-eip155")]
use x402_chain_eip155::chain::config::{Eip155ChainConfig, Eip155ChainConfigInner};
#[cfg(feature = "chain-solana")]
use x402_chain_solana::chain as solana;
#[cfg(feature = "chain-solana")]
use x402_chain_solana::chain::config::{SolanaChainConfig, SolanaChainConfigInner};

/// Server configuration.
///
/// Fields use serde defaults that fall back to environment variables,
/// then to hardcoded defaults.
pub type Config = x402_types::config::Config<ChainsConfig>;

// FIXME Move to facilitator local

/// Configuration for a specific chain.
///
/// This enum represents chain-specific configuration that varies by chain family
/// (EVM vs Solana vs Aptos). The chain family is determined by the CAIP-2 prefix of the
/// chain identifier key (e.g., "eip155:" for EVM, "solana:" for Solana, "aptos:" for Aptos).
#[derive(Debug, Clone)]
pub enum ChainConfig {
    /// EVM chain configuration (for chains with "eip155:" prefix).
    #[cfg(feature = "chain-eip155")]
    Eip155(Box<Eip155ChainConfig>),
    /// Solana chain configuration (for chains with "solana:" prefix).
    #[cfg(feature = "chain-solana")]
    Solana(Box<SolanaChainConfig>),
    /// Aptos chain configuration (for chains with "aptos:" prefix).
    #[cfg(feature = "chain-aptos")]
    Aptos(Box<AptosChainConfig>),
}

/// Configuration for chains.
///
/// This is a wrapper around `Vec<ChainConfig>` that provides custom serialization
/// as a map where keys are CAIP-2 chain identifiers.
#[derive(Debug, Clone, Default)]
pub struct ChainsConfig(pub Vec<ChainConfig>);

impl Deref for ChainsConfig {
    type Target = Vec<ChainConfig>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Serialize for ChainsConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeMap;

        let chains = &self.0;
        #[allow(unused_mut)] // For when no chain features enabled
        let mut map = serializer.serialize_map(Some(chains.len()))?;
        for chain_config in chains {
            match chain_config {
                #[cfg(feature = "chain-eip155")]
                ChainConfig::Eip155(config) => {
                    let chain_id = config.chain_id();
                    let inner = &config.inner;
                    map.serialize_entry(&chain_id, inner)?;
                }
                #[cfg(feature = "chain-solana")]
                ChainConfig::Solana(config) => {
                    let chain_id = config.chain_id();
                    let inner = &config.inner;
                    map.serialize_entry(&chain_id, inner)?;
                }
                #[cfg(feature = "chain-aptos")]
                ChainConfig::Aptos(config) => {
                    let chain_id = config.chain_id();
                    let inner = &config.inner;
                    map.serialize_entry(&chain_id, inner)?;
                }
                #[allow(unreachable_patterns)] // For when no chain features enabled
                _ => unreachable!("ChainConfig variant not enabled in this build"),
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for ChainsConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::{MapAccess, Visitor};
        use std::fmt;

        struct ChainsVisitor;

        impl<'de> Visitor<'de> for ChainsVisitor {
            type Value = ChainsConfig;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a map of chain identifiers to chain configurations")
            }

            fn visit_map<M>(self, mut access: M) -> Result<Self::Value, M::Error>
            where
                M: MapAccess<'de>,
            {
                #[allow(unused_mut)] // For when no chain features enabled
                let mut chains = Vec::with_capacity(access.size_hint().unwrap_or(0));

                while let Some(chain_id) = access.next_key::<ChainId>()? {
                    let namespace = chain_id.namespace();
                    #[allow(unused_variables)] // For when no chain features enabled
                    let config = match namespace {
                        #[cfg(feature = "chain-eip155")]
                        eip155::EIP155_NAMESPACE => {
                            let inner: Eip155ChainConfigInner = access.next_value()?;
                            let config = Eip155ChainConfig {
                                chain_reference: chain_id
                                    .try_into()
                                    .map_err(|e| serde::de::Error::custom(format!("{}", e)))?,
                                inner,
                            };
                            ChainConfig::Eip155(Box::new(config))
                        }
                        #[cfg(feature = "chain-solana")]
                        solana::SOLANA_NAMESPACE => {
                            let inner: SolanaChainConfigInner = access.next_value()?;
                            let config = SolanaChainConfig {
                                chain_reference: chain_id
                                    .try_into()
                                    .map_err(|e| serde::de::Error::custom(format!("{}", e)))?,
                                inner,
                            };
                            ChainConfig::Solana(Box::new(config))
                        }
                        #[cfg(feature = "chain-aptos")]
                        aptos::APTOS_NAMESPACE => {
                            let inner: AptosChainConfigInner = access.next_value()?;
                            let config = AptosChainConfig {
                                chain_reference: chain_id
                                    .try_into()
                                    .map_err(|e| serde::de::Error::custom(format!("{}", e)))?,
                                inner,
                            };
                            ChainConfig::Aptos(Box::new(config))
                        }
                        _ => {
                            return Err(serde::de::Error::custom(format!(
                                "Unexpected namespace: {}",
                                namespace
                            )));
                        }
                    };
                    #[allow(unreachable_code)] // For when no chain features enabled
                    chains.push(config)
                }

                Ok(ChainsConfig(chains))
            }
        }

        deserializer.deserialize_map(ChainsVisitor)
    }
}
