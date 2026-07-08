//! TRON chain provider for x402 payments.
//!
//! Communicates with the TRON blockchain via the TronGrid HTTP API using
//! `visible: true`, which means all addresses are passed and returned as
//! Base58Check strings (the canonical TRON format).

use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::SolCall;
use k256::ecdsa::{RecoveryId, SigningKey, VerifyingKey};
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use x402_types::chain::{ChainId, ChainProviderOps, FromConfig};

use crate::chain::TronAddress;
use crate::chain::config::{TronChainConfig, TronPrivateKey};
use crate::chain::contracts;
use crate::chain::tron_grid::{
    HexBytesVec, TronGridHttp, TronGridLike, TronGridLikeError, TronGridPolling, WaitForTxLike,
};
use crate::chain::types::{TronChainReference, TronTxId};

// ── Errors ────────────────────────────────────────────────────────────────────

/// Errors that can occur while talking to TronGrid or submitting transactions.
#[derive(Debug, thiserror::Error)]
pub enum TronChainProviderError {
    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("TronGrid API error: {0}")]
    Api(String),
    #[error("ABI decode error: {0}")]
    AbiDecode(String),
    #[error("Invalid private key: {0}")]
    InvalidKey(String),
    #[error("Transaction failed: {0}")]
    TxFailed(String),
    #[error("Transaction timed out")]
    TxTimeout,
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error(transparent)]
    TronGrid(#[from] TronGridLikeError),
}

impl From<TronChainProviderError> for x402_types::scheme::X402SchemeFacilitatorError {
    fn from(e: TronChainProviderError) -> Self {
        Self::OnchainFailure(e.to_string())
    }
}

impl From<TronChainProviderError> for x402_types::proto::PaymentVerificationError {
    fn from(e: TronChainProviderError) -> Self {
        Self::TransactionSimulation(e.to_string())
    }
}

/// A single k256 signing key paired with its derived TRON address.
pub struct TronSigner {
    signing_key: SigningKey,
    address: TronAddress,
}

impl TronSigner {
    /// Returns the TRON address derived from this signer's public key.
    pub fn address(&self) -> TronAddress {
        self.address
    }

    /// Derives a `TronSigner` from a private key (TRON uses the same secp256k1 + keccak
    /// derivation as EVM chains).
    pub fn from_key(key: &TronPrivateKey) -> Result<Self, TronChainProviderError> {
        let signing_key = SigningKey::from(key.clone());
        let verifying_key = VerifyingKey::from(&signing_key);
        let point = verifying_key.to_encoded_point(false);
        let pub_bytes = &point.as_bytes()[1..]; // strip 0x04 prefix
        let hash = alloy_primitives::keccak256(pub_bytes);
        let evm_address = Address::from_slice(&hash[12..]);
        let tron_address = TronAddress::from(evm_address);
        Ok(Self {
            signing_key,
            address: tron_address,
        })
    }
}

impl TronSigner {
    /// Sign a transaction hash. Format: r(32) + s(32) + (recovery_id + 27)(1).
    pub fn sign(&self, tx_id: &TronTxId) -> Result<[u8; 65], TronChainProviderError> {
        let (sig, recid): (k256::ecdsa::Signature, RecoveryId) = self
            .signing_key
            .sign_prehash_recoverable(&tx_id.0)
            .map_err(|e| TronChainProviderError::InvalidKey(format!("sign failed: {e}")))?;
        let mut sig_bytes = [0u8; 65];
        sig_bytes[..64].copy_from_slice(&sig.to_bytes());
        sig_bytes[64] = recid.to_byte() + 27;
        Ok(sig_bytes)
    }
}

impl Debug for TronSigner {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "TronSigner {{ address: {:?} }}", self.address)
    }
}

impl Display for TronSigner {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.address)
    }
}

// ── TronSigners ───────────────────────────────────────────────────────────────

/// A pool of configured facilitator signers, dispatched round-robin.
pub struct TronSigners {
    inner: Vec<TronSigner>,
    next_idx: AtomicUsize,
}

impl TronSigners {
    /// Creates a signer pool from an already-derived list of signers.
    pub fn new(signers: Vec<TronSigner>) -> Self {
        Self {
            inner: signers,
            next_idx: AtomicUsize::new(0),
        }
    }

    /// Returns the next signer in round-robin order.
    pub fn next(&self) -> &TronSigner {
        let idx = self.next_idx.fetch_add(1, Ordering::Relaxed) % self.inner.len();
        &self.inner[idx]
    }

    /// Returns the signer for the given address, or an error if not configured.
    pub fn get(&self, addr: &TronAddress) -> Result<&TronSigner, TronChainProviderError> {
        self.inner
            .iter()
            .find(|s| s.address == *addr)
            .ok_or_else(|| TronChainProviderError::InvalidKey(format!("no signer for {addr}")))
    }

    /// Returns whether the given address is one of the configured signers.
    pub fn contains(&self, addr: &TronAddress) -> bool {
        self.inner.iter().any(|s| s.address == *addr)
    }

    /// Returns the addresses of all configured signers.
    pub fn addresses(&self) -> impl Iterator<Item = &TronAddress> {
        self.inner.iter().map(|s| &s.address)
    }
}

impl fmt::Debug for TronSigners {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.inner.iter().map(|s| s.address.to_string()))
            .finish()
    }
}

// ── TronChainProvider ─────────────────────────────────────────────────────────

/// TRON chain provider.
///
/// Wraps TronGrid HTTP API (`visible: true`) and one or more k256 signing keys.
pub struct TronChainProvider {
    /// Chain reference for this provider.
    pub chain_reference: TronChainReference,
    /// TronGrid client
    pub tron_grid: TronGridPolling<TronGridHttp>,
    /// Chain-specific addresses.
    pub addresses: TronChainAddresses,
    /// All configured signers (at least one required).
    signers: TronSigners,
}

impl TronChainProvider {
    /// Replaces the TronGrid client (e.g. to inject a mock in tests).
    pub fn with_tron_grid(mut self, tron_grid: TronGridPolling<TronGridHttp>) -> Self {
        self.tron_grid = tron_grid;
        self
    }
}

impl fmt::Debug for TronChainProvider {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("TronChainProvider")
            .field("chain_reference", &self.chain_reference)
            .field("tron_grid", &self.tron_grid)
            .field("signers", &self.signers)
            .finish()
    }
}

#[async_trait::async_trait]
impl FromConfig<TronChainConfig> for TronChainProvider {
    async fn from_config(config: &TronChainConfig) -> Result<Self, Box<dyn std::error::Error>> {
        let signers = &config.inner.signers;
        if signers.is_empty() {
            return Err(TronChainProviderError::InvalidKey(
                "at least one signer is required".to_string(),
            )
            .into());
        }
        let signers = signers
            .iter()
            .map(|k| TronSigner::from_key(k))
            .collect::<Result<Vec<_>, _>>()
            .map(TronSigners::new)?;

        // Explicit config overrides the well-known default
        let chain_reference = config.chain_reference;
        let contracts = config.inner.contracts.as_ref();
        let x402_exact_permit2_proxy = contracts
            .and_then(|c| c.x402_exact_permit2_proxy)
            .or_else(|| chain_reference.x402_exact_permit2_proxy())
            .ok_or(TronChainProviderError::Api(format!(
                "can not get x402ExactPermit2Proxy contract address for tron:{chain_reference}"
            )))?;
        let sun_permit2 = contracts
            .and_then(|c| c.sun_permit2)
            .or_else(|| chain_reference.sun_permit2())
            .ok_or(TronChainProviderError::Api(format!(
                "can not get Permit2 contract address for tron:{chain_reference}"
            )))?;

        let tron_grid = {
            let rpc_url = config.inner.rpc_url.inner().clone();
            let tron_grid = TronGridHttp::new(rpc_url);
            TronGridPolling {
                tron_grid,
                tx_timeout: config.inner.tx_timeout(),
                tx_poll_interval: config.inner.tx_poll_interval(),
            }
        };

        let addresses = TronChainAddresses {
            sun_permit2,
            x402_exact_permit2_proxy,
        };

        Ok(Self {
            chain_reference,
            signers,
            tron_grid,
            addresses,
        })
    }
}

impl ChainProviderOps for TronChainProvider {
    fn signer_addresses(&self) -> Vec<String> {
        self.signers.addresses().map(|a| a.to_string()).collect()
    }

    fn chain_id(&self) -> ChainId {
        self.chain_reference.chain_id()
    }
}

/// Accessors for the contract addresses a TRON chain provider was configured with.
pub trait TronChainAddressesLike {
    /// The SUN.io Permit2 contract (the EIP-712 `verifyingContract` clients sign against).
    fn sun_permit2(&self) -> TronAddress;
    /// The x402ExactPermit2Proxy contract (the Permit2 `spender` and settlement target).
    fn x402_exact_permit2_proxy(&self) -> TronAddress;
}

/// Contract addresses resolved for a given TRON chain (from config or well-known defaults).
pub struct TronChainAddresses {
    /// SUN.io Permit2 contract — the EIP-712 `verifyingContract` that clients sign against.
    pub sun_permit2: TronAddress,
    /// x402ExactPermit2Proxy — the `spender` in Permit2 messages and the settlement contract.
    pub x402_exact_permit2_proxy: TronAddress,
}

impl TronChainAddressesLike for TronChainAddresses {
    fn sun_permit2(&self) -> TronAddress {
        self.sun_permit2
    }

    fn x402_exact_permit2_proxy(&self) -> TronAddress {
        self.x402_exact_permit2_proxy
    }
}

/// Chain access needed by facilitators: read-only calls, signing, and settlement.
///
/// Implemented directly by [`TronChainProvider`] and, transparently, by `Arc<T>` so
/// facilitators can be generic over either.
pub trait TronChainProviderLike {
    /// Concrete address bundle type returned by [`Self::addresses`].
    type Addresses: TronChainAddressesLike;
    /// Contract addresses configured for this chain.
    fn addresses(&self) -> &Self::Addresses;
    /// Whether `addr` is one of the facilitator's own signers (must never be a payer).
    fn is_signer(&self, addr: &TronAddress) -> bool;
    /// The TRON chain this provider is connected to.
    fn chain(&self) -> &TronChainReference;
    /// Calls a read-only (constant) contract method via `triggerconstantcontract`.
    fn trigger_constant_contract<TCalldata>(
        &self,
        contract: TronAddress,
        calldata: TCalldata,
        from: Option<TronAddress>,
    ) -> impl Future<Output = Result<TCalldata::Return, TronChainProviderError>> + Send
    where
        TCalldata: SolCall + Send;
    /// Builds, signs (round-robin or with a specific `from` signer), and broadcasts a
    /// contract call, returning the resulting transaction ID.
    fn build_and_submit_tx<TCalldata>(
        &self,
        contract: TronAddress,
        calldata: TCalldata,
        from: Option<TronAddress>,
    ) -> impl Future<Output = Result<TronTxId, TronChainProviderError>> + Send
    where
        TCalldata: SolCall + Send;
}

impl<T> TronChainProviderLike for Arc<T>
where
    T: TronChainProviderLike,
{
    type Addresses = T::Addresses;

    fn addresses(&self) -> &Self::Addresses {
        self.as_ref().addresses()
    }

    fn is_signer(&self, addr: &TronAddress) -> bool {
        self.as_ref().is_signer(addr)
    }

    fn chain(&self) -> &TronChainReference {
        self.as_ref().chain()
    }

    fn trigger_constant_contract<TCalldata>(
        &self,
        contract: TronAddress,
        calldata: TCalldata,
        from: Option<TronAddress>,
    ) -> impl Future<Output = Result<TCalldata::Return, TronChainProviderError>> + Send
    where
        TCalldata: SolCall + Send,
    {
        self.as_ref()
            .trigger_constant_contract(contract, calldata, from)
    }

    fn build_and_submit_tx<TCalldata>(
        &self,
        contract: TronAddress,
        calldata: TCalldata,
        from: Option<TronAddress>,
    ) -> impl Future<Output = Result<TronTxId, TronChainProviderError>> + Send
    where
        TCalldata: SolCall + Send,
    {
        self.as_ref().build_and_submit_tx(contract, calldata, from)
    }
}

impl TronChainProviderLike for TronChainProvider {
    type Addresses = TronChainAddresses;

    fn addresses(&self) -> &Self::Addresses {
        &self.addresses
    }

    fn is_signer(&self, addr: &TronAddress) -> bool {
        self.signers.contains(addr)
    }

    fn chain(&self) -> &TronChainReference {
        &self.chain_reference
    }
    async fn trigger_constant_contract<TCalldata>(
        &self,
        contract_address: TronAddress,
        calldata: TCalldata,
        from: Option<TronAddress>,
    ) -> Result<TCalldata::Return, TronChainProviderError>
    where
        TCalldata: SolCall + Send,
    {
        let decoded = self
            .tron_grid
            .wallet_trigger_constant_contract(contract_address, calldata, from)
            .await?;
        Ok(decoded)
    }

    async fn build_and_submit_tx<TCalldata>(
        &self,
        contract: TronAddress,
        calldata: TCalldata,
        from: Option<TronAddress>,
    ) -> Result<TronTxId, TronChainProviderError>
    where
        TCalldata: SolCall + Send,
    {
        let signer = match from {
            Some(addr) => self.signers.get(&addr)?,
            None => self.signers.next(),
        };
        let mut tx = self
            .tron_grid
            .wallet_trigger_smart_contract(contract, calldata, signer.address)
            .await?;

        let signature = signer.sign(&tx.tx_id)?;
        let signature = Bytes::from(signature);
        tx.signature = HexBytesVec(vec![signature]);
        let tx_id = self.tron_grid.wallet_broadcast_transaction(tx).await?;
        Ok(tx_id)
    }
}

impl WaitForTxLike for TronChainProvider {
    fn wait_for_tx(
        &self,
        tx_id: &TronTxId,
    ) -> impl Future<Output = Result<(), TronChainProviderError>> {
        self.tron_grid.wait_for_tx(tx_id)
    }
}

// ── ERC20 reads (used by both EIP-3009 and Permit2 facilitators) ──────────────

/// Reads `token.balanceOf(owner_evm)`.
pub async fn read_balance_of<P: TronChainProviderLike>(
    provider: &P,
    token: TronAddress,
    owner_evm: Address,
) -> Result<U256, TronChainProviderError> {
    provider
        .trigger_constant_contract(
            token,
            contracts::erc20::balanceOfCall { account: owner_evm },
            None,
        )
        .await
}

/// Reads `token.allowance(owner_evm, spender_evm)`.
pub async fn read_allowance<P: TronChainProviderLike>(
    provider: &P,
    token: TronAddress,
    owner_evm: Address,
    spender_evm: Address,
) -> Result<U256, TronChainProviderError> {
    provider
        .trigger_constant_contract(
            token,
            contracts::erc20::allowanceCall {
                owner: owner_evm,
                spender: spender_evm,
            },
            None,
        )
        .await
}
