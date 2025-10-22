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

use alloy::contract::SolCallBuilder;
use alloy::dyn_abi::SolType;
use alloy::network::{
    Ethereum as AlloyEthereum, EthereumWallet, NetworkWallet, TransactionBuilder,
};
use alloy::primitives::{Address, Bytes, FixedBytes, U256, address};
use alloy::providers::ProviderBuilder;
use alloy::providers::bindings::IMulticall3;
use alloy::providers::fillers::NonceManager;
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller,
};
use alloy::providers::{
    Identity, MULTICALL3_ADDRESS, MulticallItem, Provider, RootProvider, WalletProvider,
};
use alloy::rpc::client::RpcClient;
use alloy::rpc::types::{TransactionReceipt, TransactionRequest};
use alloy::sol_types::{Eip712Domain, SolCall, SolStruct, eip712_domain};
use alloy::{hex, sol};
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Mutex;
use tracing::{Instrument, instrument};
use tracing_core::Level;

use crate::chain::{FacilitatorLocalError, FromEnvByNetworkBuild, NetworkProviderOps};
use crate::facilitator::Facilitator;
use crate::from_env;
use crate::network::{Network, USDCDeployment};
use crate::timestamp::UnixTimestamp;
use crate::types::{
    EvmAddress, EvmSignature, ExactPaymentPayload, FacilitatorErrorReason, HexEncodedNonce,
    MixedAddress, PaymentPayload, PaymentRequirements, Scheme, SettleRequest, SettleResponse,
    SupportedPaymentKind, SupportedPaymentKindsResponse, TokenAmount, TransactionHash,
    TransferWithAuthorization, VerifyRequest, VerifyResponse, X402Version,
};

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
const VALIDATOR_ADDRESS: alloy::primitives::Address =
    address!("0xdAcD51A54883eb67D95FAEb2BBfdC4a9a6BD2a3B");

type InnerFiller = JoinFill<
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

/// Chain descriptor used by the EVM provider.
///
/// Wraps a `Network` enum and the concrete `chain_id` used for EIP-155 and EIP-712.
#[derive(Clone, Copy, Debug)]
pub struct EvmChain {
    /// x402 network name (Base, Avalanche, etc.).
    pub network: Network,
    /// Numeric chain id used in transactions and EIP-712 domains.
    pub chain_id: u64,
}

impl EvmChain {
    /// Construct a chain descriptor from a network and chain id.
    pub fn new(network: Network, chain_id: u64) -> Self {
        Self { network, chain_id }
    }

    pub fn network(&self) -> Network {
        self.network
    }
}

impl TryFrom<Network> for EvmChain {
    type Error = FacilitatorLocalError;

    /// Map a `Network` to its canonical `chain_id`.
    ///
    /// # Errors
    /// Returns [`FacilitatorLocalError::UnsupportedNetwork`] for non-EVM networks (e.g. Solana).
    fn try_from(value: Network) -> Result<Self, Self::Error> {
        match value {
            Network::BaseSepolia => Ok(EvmChain::new(value, 84532)),
            Network::Base => Ok(EvmChain::new(value, 8453)),
            Network::XdcMainnet => Ok(EvmChain::new(value, 50)),
            Network::AvalancheFuji => Ok(EvmChain::new(value, 43113)),
            Network::Avalanche => Ok(EvmChain::new(value, 43114)),
            Network::Solana => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
            Network::SolanaDevnet => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
            Network::PolygonAmoy => Ok(EvmChain::new(value, 80002)),
            Network::Polygon => Ok(EvmChain::new(value, 137)),
            Network::Sei => Ok(EvmChain::new(value, 1329)),
            Network::SeiTestnet => Ok(EvmChain::new(value, 1328)),
        }
    }
}

/// A fully specified ERC-3009 authorization payload for EVM settlement.
pub struct ExactEvmPayment {
    /// Target chain for settlement.
    #[allow(dead_code)] // Just in case.
    pub chain: EvmChain,
    /// Authorized sender (`from`) — EOA or smart wallet.
    pub from: EvmAddress,
    /// Authorized recipient (`to`).
    pub to: EvmAddress,
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
    inner: InnerProvider,
    eip1559: bool,
    chain: EvmChain,
    signer_addresses: Arc<Vec<Address>>,
    signer_cursor: Arc<AtomicUsize>,
}

impl EvmProvider {
    /// Build an [`EvmProvider`] from a pre-composed Alloy ethereum provider [`InnerProvider`].
    pub async fn try_new(
        wallet: EthereumWallet,
        rpc_url: &str,
        eip1559: bool,
        network: Network,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let chain = EvmChain::try_from(network)?;
        let signer_addresses: Vec<Address> =
            NetworkWallet::<AlloyEthereum>::signer_addresses(&wallet).collect();
        if signer_addresses.is_empty() {
            return Err("wallet must contain at least one signer".into());
        }
        let signer_addresses = Arc::new(signer_addresses);
        let signer_cursor = Arc::new(AtomicUsize::new(0));
        let client = RpcClient::builder()
            .connect(rpc_url)
            .await
            .map_err(|e| format!("Failed to connect to {network}: {e}"))?;
        let filler = InnerFiller::default();
        let inner = ProviderBuilder::default()
            .filler(filler)
            .wallet(wallet)
            .connect_client(client);

        tracing::info!(network=%network, rpc=rpc_url, signers=?signer_addresses, "Initialized provider");

        Ok(Self {
            inner,
            eip1559,
            chain,
            signer_addresses,
            signer_cursor,
        })
    }

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

/// Trait for sending meta-transactions with custom target and calldata.
pub trait MetaEvmProvider {
    type Error;
    type Inner: Provider;

    fn inner(&self) -> &Self::Inner;
    fn chain(&self) -> &EvmChain;

    fn send_transaction(
        &self,
        tx: MetaTransaction,
    ) -> impl Future<Output = Result<TransactionReceipt, Self::Error>> + Send;
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

impl MetaEvmProvider for EvmProvider {
    type Error = FacilitatorLocalError;
    type Inner = InnerProvider;

    fn inner(&self) -> &Self::Inner {
        &self.inner
    }

    fn chain(&self) -> &EvmChain {
        &self.chain
    }

    /// Send a meta-transaction with provided `to`, `calldata`, and automatically selected signer.
    ///
    /// This method constructs a transaction from the provided [`MetaTransaction`], automatically
    /// selects the next available signer using round-robin selection, and handles gas pricing
    /// based on whether the network supports EIP-1559.
    ///
    /// # Gas Pricing Strategy
    ///
    /// - **EIP-1559 networks**: Uses automatic gas pricing via the provider's fillers.
    /// - **Legacy networks**: Fetches the current gas price using `get_gas_price()` and sets it explicitly.
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
    /// - Receipt retrieval fails
    async fn send_transaction(
        &self,
        tx: MetaTransaction,
    ) -> Result<TransactionReceipt, Self::Error> {
        let mut txr = TransactionRequest::default()
            .with_to(tx.to)
            .with_from(self.next_signer_address())
            .with_input(tx.calldata);
        if !self.eip1559 {
            let provider = &self.inner;
            let gas: u128 = provider
                .get_gas_price()
                .instrument(tracing::info_span!("get_gas_price"))
                .await
                .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?;
            txr.set_gas_price(gas);
        }
        let pending_tx = self
            .inner
            .send_transaction(txr)
            .await
            .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?;
        pending_tx
            .with_required_confirmations(tx.confirmations)
            .get_receipt()
            .await
            .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))
    }
}

impl NetworkProviderOps for EvmProvider {
    /// Address of the default signer used by this provider (for tx sending).
    fn signer_address(&self) -> MixedAddress {
        self.inner.default_signer_address().into()
    }

    /// x402 network handled by this provider.
    fn network(&self) -> Network {
        self.chain.network
    }
}

impl FromEnvByNetworkBuild for EvmProvider {
    async fn from_env(network: Network) -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let env_var = from_env::rpc_env_name_from_network(network);
        let rpc_url = match std::env::var(env_var).ok() {
            Some(rpc_url) => rpc_url,
            None => {
                tracing::warn!(network=%network, "no RPC URL configured, skipping");
                return Ok(None);
            }
        };
        let wallet = from_env::SignerType::from_env()?.make_evm_wallet()?;
        let is_eip1559 = match network {
            Network::BaseSepolia => true,
            Network::Base => true,
            Network::XdcMainnet => false,
            Network::AvalancheFuji => true,
            Network::Avalanche => true,
            Network::Solana => false,
            Network::SolanaDevnet => false,
            Network::PolygonAmoy => true,
            Network::Polygon => true,
            Network::Sei => true,
            Network::SeiTestnet => true,
        };
        let provider = EvmProvider::try_new(wallet, &rpc_url, is_eip1559, network).await?;
        Ok(Some(provider))
    }
}

impl<P> Facilitator for P
where
    P: MetaEvmProvider + Sync,
    FacilitatorLocalError: From<P::Error>,
{
    type Error = FacilitatorLocalError;

    /// Verify x402 payment intent by simulating signature validity and ERC-3009 transfer.
    ///
    /// For EIP-6492 signatures, perform a multicall: first the validator’s
    /// `isValidSigWithSideEffects` (which *may* deploy the counterfactual wallet in sim),
    /// then the token’s `transferWithAuthorization`. Both run within a single `eth_call`
    /// so the state is shared during simulation.
    ///
    /// # Errors
    /// - [`FacilitatorLocalError::NetworkMismatch`], [`FacilitatorLocalError::SchemeMismatch`], [`FacilitatorLocalError::ReceiverMismatch`] if inputs are inconsistent.
    /// - [`FacilitatorLocalError::InvalidTiming`] if outside `validAfter/validBefore`.
    /// - [`FacilitatorLocalError::InsufficientFunds`] / `FacilitatorLocalError::InsufficientValue` on balance/value checks.
    /// - [`FacilitatorLocalError::ContractCall`] if on-chain calls revert.
    async fn verify(&self, request: &VerifyRequest) -> Result<VerifyResponse, Self::Error> {
        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;
        let (contract, payment, eip712_domain) =
            assert_valid_payment(self.inner(), self.chain(), payload, requirements).await?;

        let signed_message = SignedMessage::extract(&payment, &eip712_domain)?;
        let payer = signed_message.address;
        let hash = signed_message.hash;
        match signed_message.signature {
            StructuredSignature::EIP6492 {
                factory: _,
                factory_calldata: _,
                inner,
                original,
            } => {
                // Prepare the call to validate EIP-6492 signature
                let validator6492 = Validator6492::new(VALIDATOR_ADDRESS, self.inner());
                let is_valid_signature_call =
                    validator6492.isValidSigWithSideEffects(payer, hash, original);
                // Prepare the call to simulate transfer the funds
                let transfer_call = transferWithAuthorization_0(&contract, &payment, inner).await?;
                // Execute both calls in a single transaction simulation to accommodate for possible smart wallet creation
                let (is_valid_signature_result, transfer_result) = self
                    .inner()
                    .multicall()
                    .add(is_valid_signature_call)
                    .add(transfer_call.tx)
                    .aggregate3()
                    .instrument(tracing::info_span!("call_transferWithAuthorization_0",
                            from = %transfer_call.from,
                            to = %transfer_call.to,
                            value = %transfer_call.value,
                            valid_after = %transfer_call.valid_after,
                            valid_before = %transfer_call.valid_before,
                            nonce = %transfer_call.nonce,
                            signature = %transfer_call.signature,
                            token_contract = %transfer_call.contract_address,
                            otel.kind = "client",
                    ))
                    .await
                    .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?;
                let is_valid_signature_result = is_valid_signature_result
                    .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?;
                if !is_valid_signature_result {
                    return Err(FacilitatorLocalError::InvalidSignature(
                        payer.into(),
                        "Incorrect signature".to_string(),
                    ));
                }
                transfer_result.map_err(|e| FacilitatorLocalError::ContractCall(format!("{e}")))?;
            }
            StructuredSignature::EIP1271(signature) => {
                // It is EOA or EIP-1271 signature, which we can pass to the transfer simulation
                let transfer_call =
                    transferWithAuthorization_0(&contract, &payment, signature).await?;
                transfer_call
                    .tx
                    .call()
                    .into_future()
                    .instrument(tracing::info_span!("call_transferWithAuthorization_0",
                            from = %transfer_call.from,
                            to = %transfer_call.to,
                            value = %transfer_call.value,
                            valid_after = %transfer_call.valid_after,
                            valid_before = %transfer_call.valid_before,
                            nonce = %transfer_call.nonce,
                            signature = %transfer_call.signature,
                            token_contract = %transfer_call.contract_address,
                            otel.kind = "client",
                    ))
                    .await
                    .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?;
            }
        }

        Ok(VerifyResponse::valid(payer.into()))
    }

    /// Settle a verified payment on-chain.
    ///
    /// If the signer is counterfactual (EIP-6492) and the wallet is not yet deployed,
    /// this submits **one** transaction to Multicall3 (`aggregate3`) that:
    /// 1) calls the 6492 factory with the provided calldata (best-effort prepare),
    /// 2) calls `transferWithAuthorization` with the **inner** signature.
    ///
    /// This makes deploy + transfer atomic and avoids read-your-write issues.
    ///
    /// If the wallet is already deployed (or the signature is plain EIP-1271/EOA),
    /// we submit a single `transferWithAuthorization` transaction.
    ///
    /// # Returns
    /// A [`SettleResponse`] containing success flag and transaction hash.
    ///
    /// # Errors
    /// Propagates [`FacilitatorLocalError::ContractCall`] on deployment or transfer failures
    /// and all prior validation errors.
    async fn settle(&self, request: &SettleRequest) -> Result<SettleResponse, Self::Error> {
        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;
        let (contract, payment, eip712_domain) =
            assert_valid_payment(self.inner(), self.chain(), payload, requirements).await?;

        let signed_message = SignedMessage::extract(&payment, &eip712_domain)?;
        let payer = signed_message.address;
        let transaction_receipt_fut = match signed_message.signature {
            StructuredSignature::EIP6492 {
                factory,
                factory_calldata,
                inner,
                original: _,
            } => {
                let is_contract_deployed = is_contract_deployed(self.inner(), &payer).await?;
                let transfer_call = transferWithAuthorization_0(&contract, &payment, inner).await?;
                if is_contract_deployed {
                    // transferWithAuthorization with inner signature
                    self.send_transaction(MetaTransaction {
                        to: transfer_call.tx.target(),
                        calldata: transfer_call.tx.calldata().clone(),
                        confirmations: 1,
                    })
                    .instrument(
                        tracing::info_span!("call_transferWithAuthorization_0",
                            from = %transfer_call.from,
                            to = %transfer_call.to,
                            value = %transfer_call.value,
                            valid_after = %transfer_call.valid_after,
                            valid_before = %transfer_call.valid_before,
                            nonce = %transfer_call.nonce,
                            signature = %transfer_call.signature,
                            token_contract = %transfer_call.contract_address,
                            sig_kind="EIP6492.deployed",
                            otel.kind = "client",
                        ),
                    )
                } else {
                    // deploy the smart wallet, and transferWithAuthorization with inner signature
                    let deployment_call = IMulticall3::Call3 {
                        allowFailure: true,
                        target: factory,
                        callData: factory_calldata,
                    };
                    let transfer_with_authorization_call = IMulticall3::Call3 {
                        allowFailure: false,
                        target: transfer_call.tx.target(),
                        callData: transfer_call.tx.calldata().clone(),
                    };
                    let aggregate_call = IMulticall3::aggregate3Call {
                        calls: vec![deployment_call, transfer_with_authorization_call],
                    };
                    self.send_transaction(MetaTransaction {
                        to: MULTICALL3_ADDRESS,
                        calldata: aggregate_call.abi_encode().into(),
                        confirmations: 1,
                    })
                    .instrument(
                        tracing::info_span!("call_transferWithAuthorization_0",
                            from = %transfer_call.from,
                            to = %transfer_call.to,
                            value = %transfer_call.value,
                            valid_after = %transfer_call.valid_after,
                            valid_before = %transfer_call.valid_before,
                            nonce = %transfer_call.nonce,
                            signature = %transfer_call.signature,
                            token_contract = %transfer_call.contract_address,
                            sig_kind="EIP6492.counterfactual",
                            otel.kind = "client",
                        ),
                    )
                }
            }
            StructuredSignature::EIP1271(eip1271_signature) => {
                let transfer_call =
                    transferWithAuthorization_0(&contract, &payment, eip1271_signature).await?;
                // transferWithAuthorization with eip1271 signature
                self.send_transaction(MetaTransaction {
                    to: transfer_call.tx.target(),
                    calldata: transfer_call.tx.calldata().clone(),
                    confirmations: 1,
                })
                .instrument(
                    tracing::info_span!("call_transferWithAuthorization_0",
                        from = %transfer_call.from,
                        to = %transfer_call.to,
                        value = %transfer_call.value,
                        valid_after = %transfer_call.valid_after,
                        valid_before = %transfer_call.valid_before,
                        nonce = %transfer_call.nonce,
                        signature = %transfer_call.signature,
                        token_contract = %transfer_call.contract_address,
                        sig_kind="EIP1271",
                        otel.kind = "client",
                    ),
                )
            }
        };
        let receipt = transaction_receipt_fut.await?;
        let success = receipt.status();
        if success {
            tracing::event!(Level::INFO,
                status = "ok",
                tx = %receipt.transaction_hash,
                "transferWithAuthorization_0 succeeded"
            );
            Ok(SettleResponse {
                success: true,
                error_reason: None,
                payer: payment.from.into(),
                transaction: Some(TransactionHash::Evm(receipt.transaction_hash.0)),
                network: payload.network,
            })
        } else {
            tracing::event!(
                Level::WARN,
                status = "failed",
                tx = %receipt.transaction_hash,
                "transferWithAuthorization_0 failed"
            );
            Ok(SettleResponse {
                success: false,
                error_reason: Some(FacilitatorErrorReason::InvalidScheme),
                payer: payment.from.into(),
                transaction: Some(TransactionHash::Evm(receipt.transaction_hash.0)),
                network: payload.network,
            })
        }
    }

    /// Report payment kinds supported by this provider on its current network.
    async fn supported(&self) -> Result<SupportedPaymentKindsResponse, Self::Error> {
        let kinds = vec![SupportedPaymentKind {
            network: self.chain().network().to_string(),
            x402_version: X402Version::V1,
            scheme: Scheme::Exact,
            extra: None,
        }];
        Ok(SupportedPaymentKindsResponse { kinds })
    }
}

/// A prepared call to `transferWithAuthorization` (ERC-3009) including all derived fields.
///
/// This struct wraps the assembled call builder, making it reusable across verification
/// (`.call()`) and settlement (`.send()`) flows, along with context useful for tracing/logging.
///
/// This is created by [`EvmProvider::transferWithAuthorization_0`].
pub struct TransferWithAuthorization0Call<P> {
    /// The prepared call builder that can be `.call()`ed or `.send()`ed.
    pub tx: SolCallBuilder<P, USDC::transferWithAuthorization_0Call>,
    /// The sender (`from`) address for the authorization.
    pub from: alloy::primitives::Address,
    /// The recipient (`to`) address for the authorization.
    pub to: alloy::primitives::Address,
    /// The amount to transfer (value).
    pub value: U256,
    /// Start of the validity window (inclusive).
    pub valid_after: U256,
    /// End of the validity window (exclusive).
    pub valid_before: U256,
    /// 32-byte authorization nonce (prevents replay).
    pub nonce: FixedBytes<32>,
    /// EIP-712 signature for the transfer authorization.
    pub signature: Bytes,
    /// Address of the token contract used for this transfer.
    pub contract_address: alloy::primitives::Address,
}

/// Validates that the current time is within the `validAfter` and `validBefore` bounds.
///
/// Adds a 6-second grace buffer when checking expiration to account for latency.
///
/// # Errors
/// Returns [`FacilitatorLocalError::InvalidTiming`] if the authorization is not yet active or already expired.
/// Returns [`FacilitatorLocalError::ClockError`] if the system clock cannot be read.
#[instrument(skip_all, err)]
fn assert_time(
    payer: MixedAddress,
    valid_after: UnixTimestamp,
    valid_before: UnixTimestamp,
) -> Result<(), FacilitatorLocalError> {
    let now = UnixTimestamp::try_now().map_err(FacilitatorLocalError::ClockError)?;
    if valid_before < now + 6 {
        return Err(FacilitatorLocalError::InvalidTiming(
            payer,
            format!("Expired: now {} > valid_before {}", now + 6, valid_before),
        ));
    }
    if valid_after > now {
        return Err(FacilitatorLocalError::InvalidTiming(
            payer,
            format!("Not active yet: valid_after {valid_after} > now {now}",),
        ));
    }
    Ok(())
}

/// Checks if the payer has enough on-chain token balance to meet the `maxAmountRequired`.
///
/// Performs an `ERC20.balanceOf()` call using the USDC contract instance.
///
/// # Errors
/// Returns [`FacilitatorLocalError::InsufficientFunds`] if the balance is too low.
/// Returns [`FacilitatorLocalError::ContractCall`] if the balance query fails.
#[instrument(skip_all, err, fields(
    sender = %sender,
    max_required = %max_amount_required,
    token_contract = %usdc_contract.address()
))]
async fn assert_enough_balance<P: Provider>(
    usdc_contract: &USDC::USDCInstance<P>,
    sender: &EvmAddress,
    max_amount_required: U256,
) -> Result<(), FacilitatorLocalError> {
    let balance = usdc_contract
        .balanceOf(sender.0)
        .call()
        .into_future()
        .instrument(tracing::info_span!(
            "fetch_token_balance",
            token_contract = %usdc_contract.address(),
            sender = %sender,
            otel.kind = "client"
        ))
        .await
        .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?;

    if balance < max_amount_required {
        Err(FacilitatorLocalError::InsufficientFunds((*sender).into()))
    } else {
        Ok(())
    }
}

/// Verifies that the declared `value` in the payload is sufficient for the required amount.
///
/// This is a static check (not on-chain) that compares two numbers.
///
/// # Errors
/// Return [`FacilitatorLocalError::InsufficientValue`] if the payload's value is less than required.
#[instrument(skip_all, err, fields(
    sent = %sent,
    max_amount_required = %max_amount_required
))]
fn assert_enough_value(
    payer: &EvmAddress,
    sent: &U256,
    max_amount_required: &U256,
) -> Result<(), FacilitatorLocalError> {
    if sent < max_amount_required {
        Err(FacilitatorLocalError::InsufficientValue((*payer).into()))
    } else {
        Ok(())
    }
}

/// Check whether contract code is present at `address`.
///
/// Uses `eth_getCode` against this provider. This is useful after a counterfactual
/// deployment to confirm visibility on the sending RPC before submitting a
/// follow-up transaction.
///
/// # Errors
/// Return [`FacilitatorLocalError::ContractCall`] if the RPC call fails.
async fn is_contract_deployed<P: Provider>(
    provider: P,
    address: &Address,
) -> Result<bool, FacilitatorLocalError> {
    let bytes = provider
        .get_code_at(*address)
        .into_future()
        .instrument(tracing::info_span!("get_code_at",
            address = %address,
            otel.kind = "client",
        ))
        .await
        .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?;
    Ok(!bytes.is_empty())
}

/// Constructs the correct EIP-712 domain for signature verification.
///
/// Resolves the `name` and `version` based on:
/// - Static metadata from [`USDCDeployment`] (if available),
/// - Or by calling `version()` on the token contract if not matched statically.
#[instrument(skip_all, err, fields(
    network = %payload.network,
    asset = %asset_address
))]
async fn assert_domain<P: Provider>(
    chain: &EvmChain,
    token_contract: &USDC::USDCInstance<P>,
    payload: &PaymentPayload,
    asset_address: &Address,
    requirements: &PaymentRequirements,
) -> Result<Eip712Domain, FacilitatorLocalError> {
    let usdc = USDCDeployment::by_network(payload.network);
    let name = requirements
        .extra
        .as_ref()
        .and_then(|e| e.get("name")?.as_str().map(str::to_string))
        .or_else(|| usdc.eip712.clone().map(|e| e.name))
        .ok_or(FacilitatorLocalError::UnsupportedNetwork(None))?;
    let chain_id = chain.chain_id;
    let version = requirements
        .extra
        .as_ref()
        .and_then(|extra| extra.get("version"))
        .and_then(|version| version.as_str().map(|s| s.to_string()));
    let version = if let Some(extra_version) = version {
        Some(extra_version)
    } else if usdc.address() == (*asset_address).into() {
        usdc.eip712.clone().map(|e| e.version)
    } else {
        None
    };
    let version = if let Some(version) = version {
        version
    } else {
        token_contract
            .version()
            .call()
            .into_future()
            .instrument(tracing::info_span!(
                "fetch_eip712_version",
                otel.kind = "client",
            ))
            .await
            .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?
    };
    let domain = eip712_domain! {
        name: name,
        version: version,
        chain_id: chain_id,
        verifying_contract: *asset_address,
    };
    Ok(domain)
}

/// Runs all preconditions needed for a successful payment:
/// - Valid scheme, network, and receiver.
/// - Valid time window (validAfter/validBefore).
/// - Correct EIP-712 domain construction.
/// - Sufficient on-chain balance.
/// - Sufficient value in payload.
#[instrument(skip_all, err)]
async fn assert_valid_payment<P: Provider>(
    provider: P,
    chain: &EvmChain,
    payload: &PaymentPayload,
    requirements: &PaymentRequirements,
) -> Result<(USDC::USDCInstance<P>, ExactEvmPayment, Eip712Domain), FacilitatorLocalError> {
    let payment_payload = match &payload.payload {
        ExactPaymentPayload::Evm(payload) => payload,
        ExactPaymentPayload::Solana(_) => {
            return Err(FacilitatorLocalError::UnsupportedNetwork(None));
        }
    };
    let payer = payment_payload.authorization.from;
    if payload.network != chain.network {
        return Err(FacilitatorLocalError::NetworkMismatch(
            Some(payer.into()),
            chain.network,
            payload.network,
        ));
    }
    if requirements.network != chain.network {
        return Err(FacilitatorLocalError::NetworkMismatch(
            Some(payer.into()),
            chain.network,
            requirements.network,
        ));
    }
    if payload.scheme != requirements.scheme {
        return Err(FacilitatorLocalError::SchemeMismatch(
            Some(payer.into()),
            requirements.scheme,
            payload.scheme,
        ));
    }
    let payload_to: EvmAddress = payment_payload.authorization.to;
    let requirements_to: EvmAddress = requirements
        .pay_to
        .clone()
        .try_into()
        .map_err(|e| FacilitatorLocalError::InvalidAddress(format!("{e:?}")))?;
    if payload_to != requirements_to {
        return Err(FacilitatorLocalError::ReceiverMismatch(
            payer.into(),
            payload_to.to_string(),
            requirements_to.to_string(),
        ));
    }
    let valid_after = payment_payload.authorization.valid_after;
    let valid_before = payment_payload.authorization.valid_before;
    assert_time(payer.into(), valid_after, valid_before)?;
    let asset_address = requirements
        .asset
        .clone()
        .try_into()
        .map_err(|e| FacilitatorLocalError::InvalidAddress(format!("{e:?}")))?;
    let contract = USDC::new(asset_address, provider);

    let domain = assert_domain(chain, &contract, payload, &asset_address, requirements).await?;

    let amount_required = requirements.max_amount_required.0;
    assert_enough_balance(
        &contract,
        &payment_payload.authorization.from,
        amount_required,
    )
    .await?;
    let value: U256 = payment_payload.authorization.value.into();
    assert_enough_value(&payer, &value, &amount_required)?;

    let payment = ExactEvmPayment {
        chain: *chain,
        from: payment_payload.authorization.from,
        to: payment_payload.authorization.to,
        value: payment_payload.authorization.value,
        valid_after: payment_payload.authorization.valid_after,
        valid_before: payment_payload.authorization.valid_before,
        nonce: payment_payload.authorization.nonce,
        signature: payment_payload.signature.clone(),
    };

    Ok((contract, payment, domain))
}

/// Constructs a full `transferWithAuthorization` call for a verified payment payload.
///
/// This function prepares the transaction builder with gas pricing adapted to the network's
/// capabilities (EIP-1559 or legacy) and packages it together with signature metadata
/// into a [`TransferWithAuthorization0Call`] structure.
///
/// This function does not perform any validation — it assumes inputs are already checked.
#[allow(non_snake_case)]
async fn transferWithAuthorization_0<'a, P: Provider>(
    contract: &'a USDC::USDCInstance<P>,
    payment: &ExactEvmPayment,
    signature: Bytes,
) -> Result<TransferWithAuthorization0Call<&'a P>, FacilitatorLocalError> {
    let from: Address = payment.from.into();
    let to: Address = payment.to.into();
    let value: U256 = payment.value.into();
    let valid_after: U256 = payment.valid_after.into();
    let valid_before: U256 = payment.valid_before.into();
    let nonce = FixedBytes(payment.nonce.0);
    let tx = contract.transferWithAuthorization_0(
        from,
        to,
        value,
        valid_after,
        valid_before,
        nonce,
        signature.clone(),
    );
    Ok(TransferWithAuthorization0Call {
        tx,
        from,
        to,
        value,
        valid_after,
        valid_before,
        nonce,
        signature,
        contract_address: *contract.address(),
    })
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
        factory: alloy::primitives::Address,
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
    address: alloy::primitives::Address,
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
            from: payment.from.0,
            to: payment.to.0,
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
    nonces: Arc<DashMap<alloy::primitives::Address, Arc<Mutex<u64>>>>,
}

#[async_trait]
impl NonceManager for PendingNonceManager {
    async fn get_next_nonce<P, N>(
        &self,
        provider: &P,
        address: alloy::primitives::Address,
    ) -> alloy::transports::TransportResult<u64>
    where
        P: Provider<N>,
        N: alloy::network::Network,
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
