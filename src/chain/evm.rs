//! x402 EVM flow: verification (off-chain) and settlement (on-chain).
//!
//! - **Verify**: simulate signature validity and transfer atomically in a single `eth_call`.
//!   For 6492 signatures, we call the universal validator which may *prepare* (deploy) the
//!   counterfactual wallet inside the same simulation.
//! - **Settle**: if the signer wallet is not yet deployed, we deploy it (via the 6492
//!   factory+calldata) and then call ERC-3009 `transferWithAuthorization` in a real tx.
//!
//! Assumptions:
//! - Target tokens implement ERC-3009 and support ERC-1271 for contract signers.
//! - The validator contract exists at [`VALIDATOR_ADDRESS`] on supported chains.
//!
//! Invariants:
//! - Settlement is atomic: deploy (if needed) + transfer happen in a single user flow.
//! - Verification does not persist state.

use alloy_contract::SolCallBuilder;
use alloy_network::{Ethereum as AlloyEthereum, EthereumWallet, NetworkWallet, TransactionBuilder};
use alloy_primitives::{B256, hex};
use alloy_primitives::{Bytes, FixedBytes, U256, address};
use alloy_provider::ProviderBuilder;
use alloy_provider::bindings::IMulticall3;
use alloy_provider::fillers::NonceManager;
use alloy_provider::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller,
};
use alloy_provider::{
    Identity, MULTICALL3_ADDRESS, MulticallItem, Provider, RootProvider, WalletProvider,
};
use alloy_rpc_client::RpcClient;
use alloy_rpc_types_eth::{BlockId, TransactionReceipt, TransactionRequest};
use alloy_signer::Signer;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::SolType;
use alloy_sol_types::sol;
use alloy_sol_types::{Eip712Domain, SolCall, SolStruct, eip712_domain};
use alloy_transport::TransportResult;
use alloy_transport::layers::{FallbackLayer, ThrottleLayer};
use alloy_transport_http::Http;
use async_trait::async_trait;
use dashmap::DashMap;
use std::collections::HashMap;
use std::fmt::{Display, Formatter};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;
use tower::ServiceBuilder;
use tracing::{Instrument, instrument};
use tracing_core::Level;

use crate::chain::{FacilitatorLocalError, NetworkProviderOps};
use crate::config::Eip155ChainConfig;
use crate::facilitator::Facilitator;
use crate::network::{Network, USDCDeployment};
use crate::p1::proto;
use crate::timestamp::UnixTimestamp;
use crate::types::{
    EvmSignature, ExactPaymentPayload, FacilitatorErrorReason, HexEncodedNonce, MixedAddress,
    PaymentPayload, PaymentRequirements, Scheme, SettleRequest, SettleResponse, SupportedResponse,
    TokenAmount, TransactionHash, TransferWithAuthorization, VerifyRequest, VerifyResponse,
};

use crate::p1::chain::eip155::{Eip155ChainReference, MetaTransaction};
use crate::p1::chain::{ChainId, ChainIdError};
pub use alloy_primitives::Address;

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    #[sol(rpc)]
    USDC,
    "abi/USDC.json"
);

sol! {
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    #[sol(rpc)]
    Validator6492,
    "abi/Validator6492.json"
}

/// Signature verifier for EIP-6492, EIP-1271, EOA, universally deployed on the supported EVM chains
/// If absent on a target chain, verification will fail; you should deploy the validator there.
const VALIDATOR_ADDRESS: alloy_primitives::Address =
    address!("0xdAcD51A54883eb67D95FAEb2BBfdC4a9a6BD2a3B");

/// Combined filler type for gas, blob gas, nonce, and chain ID.
pub type InnerFiller = JoinFill<
    GasFiller,
    JoinFill<BlobGasFiller, JoinFill<NonceFiller<PendingNonceManager>, ChainIdFiller>>,
>;

/// The fully composed Ethereum provider type used in this project.
///
/// Combines multiple filler layers for gas, nonce, chain ID, blob gas, and wallet signing,
/// and wraps a [`RootProvider`] for actual JSON-RPC communication.
pub type InnerProvider = FillProvider<
    JoinFill<JoinFill<Identity, InnerFiller>, WalletFiller<EthereumWallet>>,
    RootProvider,
>;

#[derive(Debug, Copy, Clone)]
pub struct EvmChainReference(u64);

impl Into<ChainId> for EvmChainReference {
    fn into(self) -> ChainId {
        ChainId::new("eip155", self.0.to_string())
    }
}

impl Into<ChainId> for &EvmChainReference {
    fn into(self) -> ChainId {
        ChainId::new("eip155", self.0.to_string())
    }
}

impl TryFrom<ChainId> for EvmChainReference {
    type Error = ChainIdError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        if value.namespace != "eip155" {
            return Err(ChainIdError::UnexpectedNamespace(
                value.namespace,
                "eip155".into(),
            ));
        }
        let chain_id: u64 = value.reference.parse().map_err(|e| {
            ChainIdError::InvalidReference(
                value.reference,
                "eip155".into(),
                format!("{e:?}").into(),
            )
        })?;
        Ok(EvmChainReference(chain_id))
    }
}

impl EvmChainReference {
    pub fn new(chain_id: u64) -> Self {
        Self(chain_id)
    }
    pub fn inner(&self) -> u64 {
        self.0
    }
}

impl Display for EvmChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<Network> for EvmChainReference {
    type Error = FacilitatorLocalError;

    /// Map a `Network` to its canonical `chain_id`.
    ///
    /// # Errors
    /// Returns [`FacilitatorLocalError::UnsupportedNetwork`] for non-EVM networks (e.g. Solana).
    fn try_from(value: Network) -> Result<Self, Self::Error> {
        match value {
            Network::BaseSepolia => Ok(EvmChainReference::new(84532)),
            Network::Base => Ok(EvmChainReference::new(8453)),
            Network::XdcMainnet => Ok(EvmChainReference::new(50)),
            Network::AvalancheFuji => Ok(EvmChainReference::new(43113)),
            Network::Avalanche => Ok(EvmChainReference::new(43114)),
            Network::XrplEvm => Ok(EvmChainReference::new(1440000)),
            Network::Solana => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
            Network::SolanaDevnet => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
            Network::PolygonAmoy => Ok(EvmChainReference::new(80002)),
            Network::Polygon => Ok(EvmChainReference::new(137)),
            Network::Sei => Ok(EvmChainReference::new(1329)),
            Network::SeiTestnet => Ok(EvmChainReference::new(1328)),
        }
    }
}

/// A fully specified ERC-3009 authorization payload for EVM settlement.
pub struct ExactEvmPayment {
    /// Target chain for settlement.
    pub chain: Eip155ChainReference,
    /// Authorized sender (`from`) — EOA or smart wallet.
    pub from: Address,
    /// Authorized recipient (`to`).
    pub to: Address,
    /// Transfer amount (token units).
    pub value: TokenAmount,
    /// Not valid before this timestamp (inclusive).
    pub valid_after: UnixTimestamp,
    /// Not valid at/after this timestamp (exclusive).
    pub valid_before: UnixTimestamp,
    /// Unique 32-byte nonce (prevents replay).
    pub nonce: HexEncodedNonce,
    /// Raw signature bytes (EIP-1271 or EIP-6492-wrapped).
    pub signature: EvmSignature,
}

/// EVM implementation of the x402 facilitator.
///
/// Holds a composed Alloy ethereum provider [`InnerProvider`],
/// an `eip1559` toggle for gas pricing strategy, and the `EvmChain` context.
#[derive(Debug)]
pub struct EvmProvider {
    /// Composed Alloy provider with all fillers.
    inner: InnerProvider,
    props: ChainProps,
    /// Chain descriptor (network + chain ID).
    chain: Eip155ChainReference,
    /// Available signer addresses for round-robin selection.
    signer_addresses: Arc<Vec<Address>>,
    /// Current position in round-robin signer rotation.
    signer_cursor: Arc<AtomicUsize>,
    /// Nonce manager for resetting nonces on transaction failures.
    nonce_manager: PendingNonceManager,
}

#[derive(Debug)]
pub struct ChainProps {
    /// Whether network supports EIP-1559 gas pricing.
    eip1559: bool,
    flashblocks: bool,
}

impl EvmProvider {
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

        let props = ChainProps {
            eip1559: config.eip1559(),
            flashblocks: config.flashblocks(),
        };

        Ok(Self {
            inner,
            props,
            chain: config.chain_reference(),
            signer_addresses,
            signer_cursor,
            nonce_manager,
        })
    }
}

/// A structured representation of an Ethereum signature.
///
/// This enum normalizes two supported cases:
///
/// - **EIP-6492 wrapped signatures**: used for counterfactual contract wallets.
///   They include deployment metadata (factory + calldata) plus the inner
///   signature that the wallet contract will validate after deployment.
/// - **EIP-1271 signatures**: plain contract (or EOA-style) signatures.
#[derive(Debug, Clone)]
enum StructuredSignature {
    /// An EIP-6492 wrapped signature.
    EIP6492 {
        /// Factory contract that can deploy the wallet deterministically
        factory: Address,
        /// Calldata to invoke on the factory (often a CREATE2 deployment).
        factory_calldata: Bytes,
        /// Inner signature for the wallet itself, probably EIP-1271.
        inner: Bytes,
        /// Full original bytes including the 6492 wrapper and magic bytes suffix.
        original: Bytes,
    },
    /// A plain EIP-1271 or EOA signature (no 6492 wrappers).
    EIP1271(Bytes),
}

/// Canonical data required to verify a signature.
#[derive(Debug, Clone)]
struct SignedMessage {
    /// Expected signer (an EOA or contract wallet).
    address: Address,
    /// 32-byte digest that was signed (typically an EIP-712 hash).
    hash: FixedBytes<32>,
    /// Structured signature, either EIP-6492 or EIP-1271.
    signature: StructuredSignature,
}

impl SignedMessage {
    /// Construct a [`SignedMessage`] from an [`ExactEvmPayment`] and its
    /// corresponding [`Eip712Domain`].
    ///
    /// This helper ties together:
    /// - The **payment intent** (an ERC-3009 `TransferWithAuthorization` struct),
    /// - The **EIP-712 domain** used for signing,
    /// - And the raw signature bytes attached to the payment.
    ///
    /// Steps performed:
    /// 1. Build an in-memory [`TransferWithAuthorization`] struct from the
    ///    `ExactEvmPayment` fields (`from`, `to`, `value`, validity window, `nonce`).
    /// 2. Compute the **EIP-712 struct hash** for that transfer under the given
    ///    `domain`. This becomes the `hash` field of the signed message.
    /// 3. Parse the raw signature bytes into a [`StructuredSignature`], which
    ///    distinguishes between:
    ///    - EIP-1271 (plain signature), and
    ///    - EIP-6492 (counterfactual signature wrapper).
    /// 4. Assemble all parts into a [`SignedMessage`] and return it.
    ///
    /// # Errors
    ///
    /// Returns [`FacilitatorLocalError`] if:
    /// - The raw signature cannot be decoded as either EIP-1271 or EIP-6492.
    pub fn extract(
        payment: &ExactEvmPayment,
        domain: &Eip712Domain,
    ) -> Result<Self, FacilitatorLocalError> {
        let transfer_with_authorization = TransferWithAuthorization {
            from: payment.from,
            to: payment.to,
            value: payment.value.into(),
            validAfter: payment.valid_after.into(),
            validBefore: payment.valid_before.into(),
            nonce: FixedBytes(payment.nonce.0),
        };
        let eip712_hash = transfer_with_authorization.eip712_signing_hash(domain);
        let expected_address = payment.from;
        let structured_signature: StructuredSignature = payment.signature.clone().try_into()?;
        let signed_message = Self {
            address: expected_address.into(),
            hash: eip712_hash,
            signature: structured_signature,
        };
        Ok(signed_message)
    }
}

/// The fixed 32-byte magic suffix defined by [EIP-6492](https://eips.ethereum.org/EIPS/eip-6492).
///
/// Any signature ending with this constant is treated as a 6492-wrapped
/// signature; the preceding bytes are ABI-decoded as `(address factory, bytes factoryCalldata, bytes innerSig)`.
const EIP6492_MAGIC_SUFFIX: [u8; 32] =
    hex!("6492649264926492649264926492649264926492649264926492649264926492");

sol! {
    /// Solidity-compatible struct for decoding the prefix of an EIP-6492 signature.
    ///
    /// Matches the tuple `(address factory, bytes factoryCalldata, bytes innerSig)`.
    #[derive(Debug)]
    struct Sig6492 {
        address factory;
        bytes   factoryCalldata;
        bytes   innerSig;
    }
}

impl TryFrom<EvmSignature> for StructuredSignature {
    type Error = FacilitatorLocalError;
    /// Convert from an `EvmSignature` wrapper to a structured signature.
    ///
    /// This delegates to the `TryFrom<Vec<u8>>` implementation.
    fn try_from(signature: EvmSignature) -> Result<Self, Self::Error> {
        signature.0.try_into()
    }
}

impl TryFrom<Vec<u8>> for StructuredSignature {
    type Error = FacilitatorLocalError;

    /// Parse raw signature bytes into a `StructuredSignature`.
    ///
    /// Rules:
    /// - If the last 32 bytes equal [`EIP6492_MAGIC_SUFFIX`], the prefix is
    ///   decoded as a [`Sig6492`] struct and returned as
    ///   [`StructuredSignature::EIP6492`].
    /// - Otherwise, the bytes are returned as [`StructuredSignature::EIP1271`].
    fn try_from(bytes: Vec<u8>) -> Result<Self, Self::Error> {
        let is_eip6492 = bytes.len() >= 32 && bytes[bytes.len() - 32..] == EIP6492_MAGIC_SUFFIX;
        let signature = if is_eip6492 {
            let body = &bytes[..bytes.len() - 32];
            let sig6492 = Sig6492::abi_decode_params(body).map_err(|e| {
                FacilitatorLocalError::ContractCall(format!(
                    "Failed to decode EIP6492 signature: {e}"
                ))
            })?;
            StructuredSignature::EIP6492 {
                factory: sig6492.factory,
                factory_calldata: sig6492.factoryCalldata,
                inner: sig6492.innerSig,
                original: bytes.into(),
            }
        } else {
            StructuredSignature::EIP1271(bytes.into())
        };
        Ok(signature)
    }
}

/// A nonce manager that caches nonces locally and checks pending transactions on initialization.
///
/// This implementation attempts to improve upon Alloy's `CachedNonceManager` by using `.pending()` when
/// fetching the initial nonce, which includes pending transactions in the mempool. This prevents
/// "nonce too low" errors when the application restarts while transactions are still pending.
///
/// # How it works
///
/// - **First call for an address**: Fetches the nonce using `.pending()`, which includes
///   transactions in the mempool, not just confirmed transactions.
/// - **Subsequent calls**: Increments the cached nonce locally without querying the RPC.
/// - **Per-address tracking**: Each address has its own cached nonce, allowing concurrent
///   transaction submission from multiple addresses.
///
/// # Thread Safety
///
/// The nonce cache is shared across all clones using `Arc<DashMap>`, ensuring that concurrent
/// requests see consistent nonce values. Each address's nonce is protected by its own `Mutex`
/// to prevent race conditions during allocation.
/// ```
#[derive(Clone, Debug, Default)]
pub struct PendingNonceManager {
    /// Cache of nonces per address. Each address has its own mutex-protected nonce value.
    nonces: Arc<DashMap<Address, Arc<Mutex<u64>>>>,
}

#[async_trait]
impl NonceManager for PendingNonceManager {
    async fn get_next_nonce<P, N>(&self, provider: &P, address: Address) -> TransportResult<u64>
    where
        P: Provider<N>,
        N: alloy_network::Network,
    {
        // Use `u64::MAX` as a sentinel value to indicate that the nonce has not been fetched yet.
        const NONE: u64 = u64::MAX;

        // Locks dashmap internally for a short duration to clone the `Arc`.
        // We also don't want to hold the dashmap lock through the await point below.
        let nonce = {
            let rm = self
                .nonces
                .entry(address)
                .or_insert_with(|| Arc::new(Mutex::new(NONE)));
            Arc::clone(rm.value())
        };

        let mut nonce = nonce.lock().await;
        let new_nonce = if *nonce == NONE {
            // Initialize the nonce if we haven't seen this account before.
            tracing::trace!(%address, "fetching nonce");
            provider.get_transaction_count(address).pending().await?
        } else {
            tracing::trace!(%address, current_nonce = *nonce, "incrementing nonce");
            *nonce + 1
        };
        *nonce = new_nonce;
        Ok(new_nonce)
    }
}

impl PendingNonceManager {
    /// Resets the cached nonce for a given address, forcing a fresh query on next use.
    ///
    /// This should be called when a transaction fails, as we cannot be certain of the
    /// actual on-chain state (the transaction may or may not have reached the mempool).
    /// By resetting to the sentinel value, the next call to `get_next_nonce` will query
    /// the RPC provider using `.pending()`, which includes mempool transactions.
    pub async fn reset_nonce(&self, address: Address) {
        if let Some(nonce_lock) = self.nonces.get(&address) {
            let mut nonce = nonce_lock.lock().await;
            *nonce = u64::MAX; // NONE sentinel - will trigger fresh query
            tracing::debug!(%address, "reset nonce cache, will requery on next use");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[tokio::test]
    async fn test_reset_nonce_clears_cache() {
        let manager = PendingNonceManager::default();
        let test_address = address!("0000000000000000000000000000000000000001");

        // Manually set a nonce in the cache (simulating it was fetched)
        {
            let nonce_lock = manager
                .nonces
                .entry(test_address)
                .or_insert_with(|| Arc::new(Mutex::new(0)));
            let mut nonce = nonce_lock.lock().await;
            *nonce = 42;
        }

        // Verify nonce is cached
        {
            let nonce_lock = manager.nonces.get(&test_address).unwrap();
            let nonce = nonce_lock.lock().await;
            assert_eq!(*nonce, 42);
        }

        // Reset the nonce
        manager.reset_nonce(test_address).await;

        // Verify nonce is reset to sentinel value (u64::MAX)
        {
            let nonce_lock = manager.nonces.get(&test_address).unwrap();
            let nonce = nonce_lock.lock().await;
            assert_eq!(*nonce, u64::MAX);
        }
    }

    #[tokio::test]
    async fn test_reset_nonce_after_allocation_sequence() {
        let manager = PendingNonceManager::default();
        let test_address = address!("0000000000000000000000000000000000000002");

        // Simulate nonce allocations
        {
            let nonce_lock = manager
                .nonces
                .entry(test_address)
                .or_insert_with(|| Arc::new(Mutex::new(0)));
            let mut nonce = nonce_lock.lock().await;
            *nonce = 50; // First allocation
            *nonce = 51; // Second allocation
            *nonce = 52; // Third allocation
        }

        // Simulate a transaction failure - reset nonce
        manager.reset_nonce(test_address).await;

        // Verify nonce is back to sentinel for requery
        {
            let nonce_lock = manager.nonces.get(&test_address).unwrap();
            let nonce = nonce_lock.lock().await;
            assert_eq!(*nonce, u64::MAX);
        }
    }

    #[tokio::test]
    async fn test_reset_nonce_on_nonexistent_address() {
        let manager = PendingNonceManager::default();
        let test_address = address!("0000000000000000000000000000000000000099");

        // Reset should not panic on address that hasn't been used
        manager.reset_nonce(test_address).await;

        // Verify nonce map still doesn't have this address
        assert!(!manager.nonces.contains_key(&test_address));
    }

    #[tokio::test]
    async fn test_multiple_addresses_independent_nonces() {
        let manager = PendingNonceManager::default();
        let address1 = address!("0000000000000000000000000000000000000001");
        let address2 = address!("0000000000000000000000000000000000000002");

        // Set nonces for both addresses
        {
            let nonce_lock1 = manager
                .nonces
                .entry(address1)
                .or_insert_with(|| Arc::new(Mutex::new(0)));
            *nonce_lock1.lock().await = 10;

            let nonce_lock2 = manager
                .nonces
                .entry(address2)
                .or_insert_with(|| Arc::new(Mutex::new(0)));
            *nonce_lock2.lock().await = 20;
        }

        // Reset address1
        manager.reset_nonce(address1).await;

        // address1 should be reset, address2 should be unchanged
        {
            let nonce_lock1 = manager.nonces.get(&address1).unwrap();
            assert_eq!(*nonce_lock1.lock().await, u64::MAX);

            let nonce_lock2 = manager.nonces.get(&address2).unwrap();
            assert_eq!(*nonce_lock2.lock().await, 20);
        }
    }

    #[tokio::test]
    async fn test_concurrent_reset_and_access() {
        let manager = Arc::new(PendingNonceManager::default());
        let test_address = address!("0000000000000000000000000000000000000003");

        // Set initial nonce
        {
            let nonce_lock = manager
                .nonces
                .entry(test_address)
                .or_insert_with(|| Arc::new(Mutex::new(0)));
            *nonce_lock.lock().await = 100;
        }

        // Spawn concurrent tasks
        let manager1 = Arc::clone(&manager);
        let handle1 = tokio::spawn(async move {
            manager1.reset_nonce(test_address).await;
        });

        let manager2 = Arc::clone(&manager);
        let handle2 = tokio::spawn(async move {
            manager2.reset_nonce(test_address).await;
        });

        // Wait for both to complete
        handle1.await.unwrap();
        handle2.await.unwrap();

        // Verify nonce is reset (both resets should work fine)
        {
            let nonce_lock = manager.nonces.get(&test_address).unwrap();
            assert_eq!(*nonce_lock.lock().await, u64::MAX);
        }
    }
}
