//! EVM chain support for x402 payments via EIP-155.
//!
//! This module provides types and providers for interacting with EVM-compatible blockchains
//! in the x402 protocol. It supports ERC-3009 `transferWithAuthorization` for gasless
//! token transfers, which is the foundation of x402 payments on EVM chains.
//!
//! # Key Types
//!
//! - [`Eip155ChainReference`] - A numeric chain ID for EVM networks (e.g., `8453` for Base)
//! - [`Eip155ChainProvider`] - Provider for interacting with EVM chains
//! - [`Eip155TokenDeployment`] - Token deployment information including address and decimals
//! - [`MetaTransaction`] - Parameters for sending meta-transactions
//!
//! # Submodules
//!
//! - [`types`] - Wire format types like [`ChecksummedAddress`](types::ChecksummedAddress) and [`TokenAmount`](types::TokenAmount)
//! - [`pending_nonce_manager`] - Nonce management for concurrent transaction submission
//!
//! # ERC-3009 Support
//!
//! The x402 protocol uses ERC-3009 `transferWithAuthorization` for payments. This allows
//! users to sign payment authorizations off-chain, which the facilitator then submits
//! on-chain. The facilitator pays the gas fees and is reimbursed through the payment.
//!
//! # Example
//!
//! ```ignore
//! use x402_rs::chain::eip155::{Eip155ChainReference, Eip155TokenDeployment};
//! use x402_rs::networks::{KnownNetworkEip155, USDC};
//!
//! // Get USDC deployment on Base
//! let usdc = USDC::base();
//! assert_eq!(usdc.decimals, 6);
//!
//! // Parse a human-readable amount
//! let amount = usdc.parse("10.50").unwrap();
//! // amount.amount is now 10_500_000 (10.50 * 10^6)
//! ```

pub mod pending_nonce_manager;
pub mod types;

use alloy_network::{Ethereum as AlloyEthereum, EthereumWallet, NetworkWallet, TransactionBuilder};
use alloy_primitives::{Address, B256, Bytes, U256};
use alloy_provider::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller,
};
use alloy_provider::{
    Identity, PendingTransactionError, Provider, ProviderBuilder, RootProvider, WalletProvider,
};
use alloy_rpc_client::RpcClient;
use alloy_rpc_types_eth::{BlockId, TransactionReceipt, TransactionRequest};
use alloy_signer::Signer;
use alloy_signer_local::PrivateKeySigner;
use alloy_transport::TransportError;
use alloy_transport::layers::{FallbackLayer, ThrottleLayer};
use alloy_transport_http::Http;
use serde::{Deserialize, Serialize};
use std::fmt::{Display, Formatter};
use std::num::NonZeroUsize;
use std::ops::Mul;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceBuilder;
use tracing::Instrument;

use crate::chain::{
    ChainId, ChainProvider, ChainProviderOps, DeployedTokenAmount, FromChainProvider, FromConfig,
};
use crate::config::Eip155ChainConfig;
use crate::util::money_amount::{MoneyAmount, MoneyAmountParseError};
pub use pending_nonce_manager::*;
pub use types::*;

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

/// The CAIP-2 namespace for EVM-compatible chains.
pub const EIP155_NAMESPACE: &str = "eip155";

/// A numeric chain ID for EVM-compatible networks.
///
/// This type wraps the numeric chain ID used by EVM networks (e.g., `1` for Ethereum mainnet,
/// `8453` for Base). It can be converted to/from a [`ChainId`] for use with the x402 protocol.
///
/// # Example
///
/// ```
/// use x402_rs::chain::eip155::Eip155ChainReference;
/// use x402_rs::chain::ChainId;
///
/// let base = Eip155ChainReference::new(8453);
/// let chain_id: ChainId = base.into();
/// assert_eq!(chain_id.to_string(), "eip155:8453");
/// ```
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct Eip155ChainReference(u64);

impl Eip155ChainReference {
    /// Converts this chain reference to a CAIP-2 [`ChainId`].
    pub fn as_chain_id(&self) -> ChainId {
        ChainId::new(EIP155_NAMESPACE, self.0.to_string())
    }
}

impl From<Eip155ChainReference> for ChainId {
    fn from(value: Eip155ChainReference) -> Self {
        ChainId::new(EIP155_NAMESPACE, value.0.to_string())
    }
}

impl From<&Eip155ChainReference> for ChainId {
    fn from(value: &Eip155ChainReference) -> Self {
        ChainId::new(EIP155_NAMESPACE, value.0.to_string())
    }
}

impl TryFrom<ChainId> for Eip155ChainReference {
    type Error = Eip155ChainReferenceFormatError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        if value.namespace != EIP155_NAMESPACE {
            return Err(Eip155ChainReferenceFormatError::InvalidNamespace(
                value.namespace,
            ));
        }
        let chain_id: u64 = value.reference.parse().map_err(|_| {
            Eip155ChainReferenceFormatError::InvalidReference(value.reference.clone())
        })?;
        Ok(Eip155ChainReference(chain_id))
    }
}

impl TryFrom<&ChainId> for Eip155ChainReference {
    type Error = Eip155ChainReferenceFormatError;

    fn try_from(value: &ChainId) -> Result<Self, Self::Error> {
        if value.namespace != EIP155_NAMESPACE {
            return Err(Eip155ChainReferenceFormatError::InvalidNamespace(
                value.namespace.clone(),
            ));
        }
        let chain_id: u64 = value.reference.parse().map_err(|_| {
            Eip155ChainReferenceFormatError::InvalidReference(value.reference.clone())
        })?;
        Ok(Eip155ChainReference(chain_id))
    }
}

/// Error returned when converting a [`ChainId`] to an [`Eip155ChainReference`].
#[derive(Debug, thiserror::Error)]
pub enum Eip155ChainReferenceFormatError {
    /// The chain ID namespace is not `eip155`.
    #[error("Invalid namespace {0}, expected eip155")]
    InvalidNamespace(String),
    /// The chain reference is not a valid numeric value.
    #[error("Invalid eip155 chain reference {0}")]
    InvalidReference(String),
}

impl Eip155ChainReference {
    /// Creates a new chain reference from a numeric chain ID.
    pub fn new(chain_id: u64) -> Self {
        Self(chain_id)
    }

    /// Returns the numeric chain ID.
    pub fn inner(&self) -> u64 {
        self.0
    }
}

impl Display for Eip155ChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Information about a token deployment on an EVM chain.
///
/// This type contains all the information needed to interact with a token contract,
/// including its address, decimal places, and optional EIP-712 domain parameters
/// for signature verification.
///
/// # Example
///
/// ```ignore
/// use x402_rs::networks::{KnownNetworkEip155, USDC};
///
/// // Get USDC deployment on Base
/// let usdc = USDC::base();
/// assert_eq!(usdc.decimals, 6);
///
/// // Parse a human-readable amount to token units
/// let amount = usdc.parse("10.50").unwrap();
/// assert_eq!(amount.amount, U256::from(10_500_000u64));
/// ```
#[derive(Debug, Clone, Eq, PartialEq, Hash)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct Eip155TokenDeployment {
    /// The chain this token is deployed on.
    pub chain_reference: Eip155ChainReference,
    /// The token contract address.
    pub address: Address,
    /// Number of decimal places for the token (e.g., 6 for USDC, 18 for most ERC-20s).
    pub decimals: u8,
    /// Optional EIP-712 domain parameters for signature verification.
    pub eip712: Option<TokenDeploymentEip712>,
}

#[allow(dead_code)] // Public for consumption by downstream crates.
impl Eip155TokenDeployment {
    /// Creates a token amount from a raw value.
    ///
    /// The value should already be in the token's smallest unit (e.g., wei).
    pub fn amount<V: Into<TokenAmount>>(
        &self,
        v: V,
    ) -> DeployedTokenAmount<U256, Eip155TokenDeployment> {
        DeployedTokenAmount {
            amount: v.into().0,
            token: self.clone(),
        }
    }

    /// Parses a human-readable amount string into token units.
    ///
    /// Accepts formats like `"10.50"`, `"$10.50"`, `"1,000"`, etc.
    /// The amount is scaled by the token's decimal places.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The input cannot be parsed as a number
    /// - The input has more decimal places than the token supports
    /// - The value is out of range
    ///
    /// # Example
    ///
    /// ```ignore
    /// use x402_rs::networks::{KnownNetworkEip155, USDC};
    ///
    /// let usdc = USDC::base();
    /// let amount = usdc.parse("10.50").unwrap();
    /// // 10.50 USDC = 10,500,000 units (6 decimals)
    /// assert_eq!(amount.amount, U256::from(10_500_000u64));
    /// ```
    pub fn parse<V>(
        &self,
        v: V,
    ) -> Result<DeployedTokenAmount<U256, Eip155TokenDeployment>, MoneyAmountParseError>
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
        let multiplier = U256::from(10).pow(U256::from(scale_diff));
        let digits = money_amount.mantissa();
        let value = U256::from(digits).mul(multiplier);
        Ok(DeployedTokenAmount {
            amount: value,
            token: self.clone(),
        })
    }
}

/// EIP-712 domain parameters for a token deployment.
///
/// These parameters are used when verifying EIP-712 typed data signatures
/// for ERC-3009 `transferWithAuthorization` calls.
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[allow(dead_code)] // Public for consumption by downstream crates.
pub struct TokenDeploymentEip712 {
    /// The token name as specified in the EIP-712 domain.
    pub name: String,
    /// The token version as specified in the EIP-712 domain.
    pub version: String,
}

/// Extractor implementation for [`ChainProvider`].
///
/// Extracts an [`Arc<Eip155ChainProvider>`] from a [`ChainProvider`] enum.
/// Returns `None` if the provider is a Solana provider.
impl FromChainProvider<ChainProvider> for Arc<Eip155ChainProvider> {
    fn from_chain_provider(provider: &ChainProvider) -> Option<Self> {
        match provider {
            ChainProvider::Eip155(p) => Some(Arc::clone(p)),
            _ => None,
        }
    }
}

/// Provider for interacting with EVM-compatible blockchains.
///
/// This provider handles:
/// - Transaction signing with multiple signers (round-robin selection)
/// - Nonce management with automatic reset on failures
/// - Gas estimation and pricing (EIP-1559 and legacy)
/// - Transaction receipt fetching with configurable timeouts
///
/// # Multiple Signers
///
/// The provider supports multiple signers for load distribution. When sending
/// transactions, signers are selected in round-robin fashion to distribute
/// the transaction load and avoid nonce conflicts.
///
/// # Nonce Management
///
/// Uses [`PendingNonceManager`] to track nonces locally and query pending
/// transactions on initialization. If a transaction fails, the nonce is
/// automatically reset to force a fresh query on the next transaction.
#[derive(Debug)]
pub struct Eip155ChainProvider {
    chain: Eip155ChainReference,
    eip1559: bool,
    flashblocks: bool,
    receipt_timeout_secs: u64,
    inner: InnerProvider,
    /// Available signer addresses for round-robin selection.
    signer_addresses: Arc<Vec<Address>>,
    /// Current position in round-robin signer rotation.
    signer_cursor: Arc<AtomicUsize>,
    /// Nonce manager for resetting nonces on transaction failures.
    nonce_manager: PendingNonceManager,
}

impl Eip155ChainProvider {
    /// Round-robin selection of next signer from wallet.
    fn next_signer_address(&self) -> Address {
        debug_assert!(!self.signer_addresses.is_empty());
        if self.signer_addresses.len() == 1 {
            self.signer_addresses[0]
        } else {
            let next =
                self.signer_cursor.fetch_add(1, Ordering::Relaxed) % self.signer_addresses.len();
            self.signer_addresses[next]
        }
    }
}

/// Creates a new provider from configuration.
///
/// Initializes signers, RPC transports, and the nonce manager.
///
/// # Errors
///
/// Returns an error if:
/// - No signers are configured
/// - Signer private keys are invalid
/// - RPC transport initialization fails
#[async_trait::async_trait]
impl FromConfig<Eip155ChainConfig> for Eip155ChainProvider {
    async fn from_config(config: &Eip155ChainConfig) -> Result<Self, Box<dyn std::error::Error>> {
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
                BlobGasFiller::default(),
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
            receipt_timeout_secs: config.receipt_timeout_secs(),
            inner,
            signer_addresses,
            signer_cursor,
            nonce_manager,
        })
    }
}

impl Eip155MetaTransactionProvider for &Eip155ChainProvider {
    type Error = MetaTransactionSendError;
    type Inner = InnerProvider;
    fn inner(&self) -> &Self::Inner {
        (*self).inner()
    }
    fn chain(&self) -> &Eip155ChainReference {
        (*self).chain()
    }
    fn send_transaction(
        &self,
        tx: MetaTransaction,
    ) -> impl Future<Output = Result<TransactionReceipt, Self::Error>> + Send {
        (*self).send_transaction(tx)
    }
}

impl Eip155MetaTransactionProvider for Arc<Eip155ChainProvider> {
    type Error = MetaTransactionSendError;
    type Inner = InnerProvider;
    fn inner(&self) -> &Self::Inner {
        (**self).inner()
    }
    fn chain(&self) -> &Eip155ChainReference {
        (**self).chain()
    }
    fn send_transaction(
        &self,
        tx: MetaTransaction,
    ) -> impl Future<Output = Result<TransactionReceipt, Self::Error>> + Send {
        (**self).send_transaction(tx)
    }
}

impl ChainProviderOps for Arc<Eip155ChainProvider> {
    fn signer_addresses(&self) -> Vec<String> {
        (**self).signer_addresses()
    }

    fn chain_id(&self) -> ChainId {
        (**self).chain_id()
    }
}

impl Eip155MetaTransactionProvider for &Arc<Eip155ChainProvider> {
    type Error = MetaTransactionSendError;
    type Inner = InnerProvider;
    fn inner(&self) -> &Self::Inner {
        (***self).inner()
    }
    fn chain(&self) -> &Eip155ChainReference {
        (***self).chain()
    }
    fn send_transaction(
        &self,
        tx: MetaTransaction,
    ) -> impl Future<Output = Result<TransactionReceipt, Self::Error>> + Send {
        (***self).send_transaction(tx)
    }
}

impl Eip155MetaTransactionProvider for Eip155ChainProvider {
    type Error = MetaTransactionSendError;
    type Inner = InnerProvider;

    fn inner(&self) -> &Self::Inner {
        &self.inner
    }

    fn chain(&self) -> &Eip155ChainReference {
        &self.chain
    }

    /// Send a meta-transaction with provided `to`, `calldata`, and automatically selected signer.
    ///
    /// This method constructs a transaction from the provided [`MetaTransaction`], automatically
    /// selects the next available signer using round-robin selection, and handles gas pricing
    /// based on whether the network supports EIP-1559.
    ///
    /// If the transaction fails at any point (during submission or receipt fetching), the nonce
    /// for the sending address is reset to force a fresh query on the next transaction. This
    /// ensures correctness even when transactions partially succeed (e.g., submitted but receipt
    /// fetch times out).
    ///
    /// # Gas Pricing Strategy
    ///
    /// - **EIP-1559 networks**: Uses automatic gas pricing via the provider's fillers.
    /// - **Legacy networks**: Fetches the current gas price using `get_gas_price()` and sets it explicitly.
    ///
    /// # Timeout Configuration
    ///
    /// Receipt fetching is subject to a configurable timeout:
    /// - Default: 30 seconds
    /// - Override via `TX_RECEIPT_TIMEOUT_SECS` environment variable
    /// - If the timeout expires, the nonce is reset and an error is returned
    ///
    /// # Parameters
    ///
    /// - `tx`: A [`MetaTransaction`] containing the target address and calldata.
    ///
    /// # Returns
    ///
    /// A [`TransactionReceipt`] once the transaction has been mined and confirmed.
    ///
    /// # Errors
    ///
    /// Returns [`FacilitatorLocalError::ContractCall`] if:
    /// - Gas price fetching fails (on legacy networks)
    /// - Transaction sending fails
    /// - Receipt retrieval fails or times out
    async fn send_transaction(
        &self,
        tx: MetaTransaction,
    ) -> Result<TransactionReceipt, Self::Error> {
        let from_address = self.next_signer_address();
        let mut txr = TransactionRequest::default()
            .with_to(tx.to)
            .with_from(from_address)
            .with_input(tx.calldata);

        if !self.eip1559 {
            let provider = &self.inner;
            let gas: u128 = provider
                .get_gas_price()
                .instrument(tracing::info_span!("get_gas_price"))
                .await?;
            txr.set_gas_price(gas);
        }

        // Estimate gas if not provided
        if txr.gas.is_none() {
            let block_id = if self.flashblocks {
                BlockId::latest()
            } else {
                BlockId::pending()
            };
            let gas_limit = self.inner.estimate_gas(txr.clone()).block(block_id).await?;
            txr.set_gas_limit(gas_limit)
        }

        // Send transaction with error handling for nonce reset
        let pending_tx = match self.inner.send_transaction(txr).await {
            Ok(pending) => pending,
            Err(e) => {
                // Transaction submission failed - reset nonce to force requery
                self.nonce_manager.reset_nonce(from_address).await;
                return Err(MetaTransactionSendError::Transport(e));
            }
        };

        // Get receipt with timeout and error handling for nonce reset
        // Default timeout of 30 seconds is reasonable for most EVM chains
        let timeout = std::time::Duration::from_secs(self.receipt_timeout_secs);

        let watcher = pending_tx
            .with_required_confirmations(tx.confirmations)
            .with_timeout(Some(timeout));

        match watcher.get_receipt().await {
            Ok(receipt) => Ok(receipt),
            Err(e) => {
                // Receipt fetch failed (timeout or other error) - reset nonce to force requery
                self.nonce_manager.reset_nonce(from_address).await;
                Err(MetaTransactionSendError::PendingTransaction(e))
            }
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum MetaTransactionSendError {
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error(transparent)]
    PendingTransaction(#[from] PendingTransactionError),
}

impl ChainProviderOps for Eip155ChainProvider {
    fn signer_addresses(&self) -> Vec<String> {
        self.inner
            .signer_addresses()
            .map(|a| a.to_string())
            .collect()
    }

    fn chain_id(&self) -> ChainId {
        self.chain.into()
    }
}

/// Meta-transaction parameters: target address, calldata, and required confirmations.
pub struct MetaTransaction {
    /// Target contract address.
    pub to: Address,
    /// Transaction calldata (encoded function call).
    pub calldata: Bytes,
    /// Number of block confirmations to wait for.
    pub confirmations: u64,
}

/// Trait for sending meta-transactions with custom target and calldata.
pub trait Eip155MetaTransactionProvider {
    /// Error type for operations.
    type Error;
    /// Underlying provider type.
    type Inner: Provider;

    /// Returns reference to underlying provider.
    fn inner(&self) -> &Self::Inner;
    /// Returns reference to chain descriptor.
    fn chain(&self) -> &Eip155ChainReference;

    /// Sends a meta-transaction to the network.
    fn send_transaction(
        &self,
        tx: MetaTransaction,
    ) -> impl Future<Output = Result<TransactionReceipt, Self::Error>> + Send;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::solana::{
        Address as SolanaAddress, SolanaChainReference, SolanaTokenDeployment,
    };
    use crate::networks::KnownNetworkSolana;
    use std::str::FromStr;

    fn create_test_deployment(decimals: u8) -> Eip155TokenDeployment {
        let chain_ref = Eip155ChainReference::new(1); // Mainnet
        Eip155TokenDeployment {
            chain_reference: chain_ref,
            address: alloy_primitives::Address::ZERO,
            decimals,
            eip712: None,
        }
    }

    #[test]
    fn test_parse_whole_number() {
        let deployment = create_test_deployment(6); // 6 decimals like USDC
        let result = deployment.parse("100");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, U256::from(100_000_000u64)); // 100 * 10^6
    }

    #[test]
    fn test_parse_with_decimals() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("1.50");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, U256::from(1_500_000u64)); // 1.50 * 10^6
    }

    #[test]
    fn test_parse_zero_decimals() {
        let deployment = create_test_deployment(0);
        let result = deployment.parse("42");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, U256::from(42u64));
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
        assert_eq!(result.unwrap().amount, U256::from(123_456_789u64));
    }

    #[test]
    fn test_parse_smallest_amount() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("0.000001");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, U256::from(1u64));
    }

    #[test]
    fn test_parse_with_currency_symbol() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("$10.50");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, U256::from(10_500_000u64));
    }

    #[test]
    fn test_parse_with_commas() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("1,000");
        assert!(result.is_ok());
        assert_eq!(result.unwrap().amount, U256::from(1_000_000_000u64));
    }

    #[test]
    fn test_parse_large_amount() {
        let deployment = create_test_deployment(6);
        let result = deployment.parse("999999999");
        assert!(result.is_ok());
        // 999999999 * 10^6 = 999999999000000
        assert_eq!(result.unwrap().amount, U256::from(999_999_999_000_000u64));
    }

    #[test]
    fn test_parse_very_large_amount_with_high_decimals() {
        // EIP155 uses U256, so we can handle much larger amounts than Solana
        let deployment = create_test_deployment(18); // 18 decimals like ETH
        let result = deployment.parse("999999999"); // 9 digits, 0 decimals
        assert!(result.is_ok());
        // 999999999 * 10^18 = 999999999000000000000000000
        let expected = U256::from(999_999_999u64) * U256::from(10).pow(U256::from(18));
        assert_eq!(result.unwrap().amount, expected);
    }

    #[test]
    fn test_parse_matches_solana_behavior() {
        // Create equivalent deployments with same decimals
        let eip155_deployment = create_test_deployment(6);

        let solana_chain = SolanaChainReference::solana();
        let solana_deployment = SolanaTokenDeployment::new(
            solana_chain,
            SolanaAddress::from_str("4zMMC9srt5Ri5X14GAgXhaHii3GnPAEERYPJgZ5nc4pb").unwrap(),
            6,
        );

        // Test various amounts
        let test_cases = ["1", "1.5", "0.01", "100", "999.999"];

        for amount in test_cases {
            let eip155_result = eip155_deployment.parse(amount);
            let solana_result = solana_deployment.parse(amount);

            assert_eq!(eip155_result.is_ok(), solana_result.is_ok());

            if let (Ok(eip155), Ok(solana)) = (eip155_result, solana_result) {
                // EIP155 uses U256, Solana uses u64
                let eip155_value: u64 = eip155.amount.try_into().unwrap();
                assert_eq!(eip155_value, solana.amount);
            }
        }
    }
}
