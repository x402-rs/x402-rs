use std::fmt::{Display, Formatter};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use alloy_network::{Ethereum as AlloyEthereum, EthereumWallet, NetworkWallet, TransactionBuilder};
use alloy_primitives::{Address, B256};
use alloy_provider::fillers::{BlobGasFiller, ChainIdFiller, GasFiller, JoinFill, NonceFiller};
use alloy_provider::{ProviderBuilder, WalletProvider};
use alloy_rpc_client::RpcClient;
use alloy_signer::Signer;
use alloy_signer_local::PrivateKeySigner;
use alloy_transport::layers::{FallbackLayer, ThrottleLayer};
use alloy_transport_http::Http;
use tower::ServiceBuilder;

use crate::chain::evm::{InnerProvider, PendingNonceManager};
use crate::config::Eip155ChainConfig;
use crate::p1::chain::{ChainId, ChainIdError, ChainProviderOps};

pub const EIP155_NAMESPACE: &str = "eip155";

#[derive(Debug, Copy, Clone)]
pub struct Eip155ChainReference(u64);

impl Into<ChainId> for Eip155ChainReference {
    fn into(self) -> ChainId {
        ChainId::new(EIP155_NAMESPACE, self.0.to_string())
    }
}

impl Into<ChainId> for &Eip155ChainReference {
    fn into(self) -> ChainId {
        ChainId::new(EIP155_NAMESPACE, self.0.to_string())
    }
}

impl TryFrom<ChainId> for Eip155ChainReference {
    type Error = ChainIdError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        if value.namespace != EIP155_NAMESPACE {
            return Err(ChainIdError::UnexpectedNamespace(
                value.namespace,
                EIP155_NAMESPACE.into(),
            ));
        }
        let chain_id: u64 = value.reference.parse().map_err(|e| {
            ChainIdError::InvalidReference(
                value.reference,
                EIP155_NAMESPACE.into(),
                format!("{e:?}").into(),
            )
        })?;
        Ok(Eip155ChainReference(chain_id))
    }
}

impl Eip155ChainReference {
    pub fn new(chain_id: u64) -> Self {
        Self(chain_id)
    }
    pub fn inner(&self) -> u64 {
        self.0
    }
}

impl Display for Eip155ChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug)]
pub struct Eip155ChainProvider {
    chain: Eip155ChainReference,
    eip1559: bool,
    flashblocks: bool,
    inner: InnerProvider,
    /// Available signer addresses for round-robin selection.
    signer_addresses: Arc<Vec<Address>>,
    /// Current position in round-robin signer rotation.
    signer_cursor: Arc<AtomicUsize>,
    /// Nonce manager for resetting nonces on transaction failures.
    nonce_manager: PendingNonceManager,
}

impl Eip155ChainProvider {
    pub async fn from_config(
        config: &Eip155ChainConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        // 1. Signers
        let signers = config
            .signers()
            .iter()
            .map(|s| B256::from_slice(s.inner().as_bytes()))
            .map(|b| {
                PrivateKeySigner::from_bytes(&b)
                    .map(|s| s.with_chain_id(Some(config.chain_reference().inner())))
            })
            .collect::<Result<Vec<_>, _>>()?;
        if signers.is_empty() {
            return Err("at least one signer should be provided".into());
        }
        let wallet = {
            let mut iter = signers.into_iter();
            let first_signer = iter
                .next()
                .expect("iterator contains at least one element by construction");
            let mut wallet = EthereumWallet::from(first_signer);
            for signer in iter {
                wallet.register_signer(signer);
            }
            wallet
        };
        let signer_addresses =
            NetworkWallet::<AlloyEthereum>::signer_addresses(&wallet).collect::<Vec<_>>();
        let signer_addresses = Arc::new(signer_addresses);
        let signer_cursor = Arc::new(AtomicUsize::new(0));

        // 2. Transports
        let transports = config
            .rpc()
            .iter()
            .filter_map(|provider_config| {
                let scheme = provider_config.http.scheme();
                let is_http = scheme == "http" || scheme == "https";
                if !is_http {
                    return None;
                }
                let rpc_url = provider_config.http.clone();
                tracing::info!(chain=%config.chain_id(), rpc_url=%rpc_url, rate_limit=?provider_config.rate_limit, "Using HTTP transport");
                let rate_limit = provider_config.rate_limit.unwrap_or(u32::MAX);
                let service = ServiceBuilder::new()
                    .layer(ThrottleLayer::new(rate_limit))
                    .service(Http::new(rpc_url));
                Some(service)
            })
            .collect::<Vec<_>>();
        let fallback = ServiceBuilder::new()
            .layer(
                FallbackLayer::default().with_active_transport_count(
                    NonZeroUsize::new(transports.len())
                        .expect("Non-zero amount of stateless transports"),
                ),
            )
            .service(transports);
        let client = RpcClient::new(fallback, false);

        // 3. Provider
        // Create nonce manager explicitly so we can store a reference for error handling
        let nonce_manager = PendingNonceManager::default();
        // Build the filler stack: Gas -> BlobGas -> Nonce -> ChainId
        // This mirrors the InnerFiller type but with our custom nonce manager
        let filler = JoinFill::new(
            GasFiller,
            JoinFill::new(
                BlobGasFiller,
                JoinFill::new(
                    NonceFiller::new(nonce_manager.clone()),
                    ChainIdFiller::default(),
                ),
            ),
        );
        let inner: InnerProvider = ProviderBuilder::default()
            .filler(filler)
            .wallet(wallet)
            .connect_client(client);

        tracing::info!(chain=%config.chain_id(), signers=?signer_addresses, "Initialized EVM provider");

        Ok(Self {
            chain: config.chain_reference(),
            eip1559: config.eip1559(),
            flashblocks: config.flashblocks(),
            inner,
            signer_addresses,
            signer_cursor,
            nonce_manager,
        })
    }
}

impl ChainProviderOps for Eip155ChainProvider {
    fn signer_addresses(&self) -> Vec<String> {
        self.inner.signer_addresses().map(|a| a.to_string()).collect()
    }

    fn chain_id(&self) -> ChainId {
        self.chain.into()
    }
}
