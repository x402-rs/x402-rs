use std::collections::HashMap;

use crate::facilitator_local::FacilitatorLocalError;
use alloy_contract::SolCallBuilder;
use alloy_primitives::{Address, B256, Bytes, U256, address, hex};
use alloy_provider::bindings::IMulticall3;
use alloy_provider::{MULTICALL3_ADDRESS, MulticallItem, Provider};
use alloy_sol_types::{Eip712Domain, SolCall, SolStruct, SolType, eip712_domain, sol};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::Instrument;
use tracing::instrument;
use tracing_core::Level;
use url::Url;

mod types;

use crate::p1::chain::eip155::{
    Eip155ChainProvider, Eip155ChainReference, MetaEip155Provider, MetaTransaction,
};
use crate::p1::chain::{ChainId, ChainProvider, ChainProviderOps};
use crate::p1::proto;
use crate::p1::scheme::{SchemeSlug, X402SchemeBlueprint, X402SchemeHandler};
use crate::timestamp::UnixTimestamp;

const SCHEME_NAME: &str = "exact";

/// Signature verifier for EIP-6492, EIP-1271, EOA, universally deployed on the supported EVM chains
/// If absent on a target chain, verification will fail; you should deploy the validator there.
const VALIDATOR_ADDRESS: Address = address!("0xdAcD51A54883eb67D95FAEb2BBfdC4a9a6BD2a3B");

pub struct V1Eip155Exact;

impl X402SchemeBlueprint for V1Eip155Exact {
    fn slug(&self) -> SchemeSlug {
        SchemeSlug::new(1, "eip155", SCHEME_NAME)
    }

    fn build(
        &self,
        provider: ChainProvider,
    ) -> Result<Box<dyn X402SchemeHandler>, Box<dyn std::error::Error>> {
        let provider = if let ChainProvider::Eip155(provider) = provider {
            provider
        } else {
            return Err("V1Eip155Exact::build: provider must be an Eip155ChainProvider".into());
        };
        Ok(Box::new(V1Eip155ExactHandler { provider }))
    }
}

pub struct V1Eip155ExactHandler {
    provider: Arc<Eip155ChainProvider>,
}

impl V1Eip155ExactHandler {
    pub fn chain_id(&self) -> ChainId {
        self.provider.chain_id()
    }
}

#[async_trait::async_trait]
impl X402SchemeHandler for V1Eip155ExactHandler {
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, FacilitatorLocalError> {
        let request = types::VerifyRequest::from_proto(request.clone()).ok_or(
            FacilitatorLocalError::DecodingError("Can not decode payload".to_string()),
        )?;
        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;
        let (contract, payment, eip712_domain) = assert_valid_payment(
            self.provider.inner(),
            self.provider.chain(),
            payload,
            requirements,
        )
        .await?;

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
                let validator6492 = Validator6492::new(VALIDATOR_ADDRESS, self.provider.inner());
                let is_valid_signature_call =
                    validator6492.isValidSigWithSideEffects(payer, hash, original);
                // Prepare the call to simulate transfer the funds
                let transfer_call = transferWithAuthorization_0(&contract, &payment, inner).await?;
                // Execute both calls in a single transaction simulation to accommodate for possible smart wallet creation
                let (is_valid_signature_result, transfer_result) = self
                    .provider
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
                        payer.to_string(),
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

        Ok(proto::VerifyResponse::valid(payer.to_string()))
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, FacilitatorLocalError> {
        let request = types::SettleRequest::from_proto(request.clone()).ok_or(
            FacilitatorLocalError::DecodingError("Can not decode payload".to_string()),
        )?;

        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;
        let (contract, payment, eip712_domain) = assert_valid_payment(
            self.provider.inner(),
            self.provider.chain(),
            payload,
            requirements,
        )
        .await?;

        let signed_message = SignedMessage::extract(&payment, &eip712_domain)?;
        let payer = signed_message.address;
        let transaction_receipt_fut = match signed_message.signature {
            StructuredSignature::EIP6492 {
                factory,
                factory_calldata,
                inner,
                original: _,
            } => {
                let is_contract_deployed =
                    is_contract_deployed(self.provider.inner(), &payer).await?;
                let transfer_call = transferWithAuthorization_0(&contract, &payment, inner).await?;
                if is_contract_deployed {
                    // transferWithAuthorization with inner signature
                    self.provider
                        .send_transaction(MetaTransaction {
                            to: transfer_call.tx.target(),
                            calldata: transfer_call.tx.calldata().clone(),
                            confirmations: 1,
                        })
                        .instrument(tracing::info_span!("call_transferWithAuthorization_0",
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
                        ))
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
                    self.provider
                        .send_transaction(MetaTransaction {
                            to: MULTICALL3_ADDRESS,
                            calldata: aggregate_call.abi_encode().into(),
                            confirmations: 1,
                        })
                        .instrument(tracing::info_span!("call_transferWithAuthorization_0",
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
                        ))
                }
            }
            StructuredSignature::EIP1271(eip1271_signature) => {
                let transfer_call =
                    transferWithAuthorization_0(&contract, &payment, eip1271_signature).await?;
                // transferWithAuthorization with eip1271 signature
                self.provider
                    .send_transaction(MetaTransaction {
                        to: transfer_call.tx.target(),
                        calldata: transfer_call.tx.calldata().clone(),
                        confirmations: 1,
                    })
                    .instrument(tracing::info_span!("call_transferWithAuthorization_0",
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
                    ))
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
            Ok(proto::SettleResponse {
                success: true,
                error_reason: None,
                payer: payment.from.to_string(),
                transaction: Some(receipt.transaction_hash.to_string()),
                network: payload.network.clone().to_string(),
            })
        } else {
            tracing::event!(
                Level::WARN,
                status = "failed",
                tx = %receipt.transaction_hash,
                "transferWithAuthorization_0 failed"
            );
            Ok(proto::SettleResponse {
                success: false,
                error_reason: Some("invalid_scheme".to_string()),
                payer: payment.from.to_string(),
                transaction: Some(receipt.transaction_hash.to_string()),
                network: payload.network.clone().to_string(),
            })
        }
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, FacilitatorLocalError> {
        let chain_id = self.chain_id();
        let kinds = {
            let mut kinds = Vec::with_capacity(2);
            kinds.push(proto::SupportedPaymentKind {
                x402_version: proto::X402Version::v2().into(),
                scheme: SCHEME_NAME.into(),
                network: chain_id.clone().into(),
                extra: None,
            });
            let network = chain_id.as_network_name();
            if let Some(network) = network {
                kinds.push(proto::SupportedPaymentKind {
                    x402_version: proto::X402Version::v1().into(),
                    scheme: SCHEME_NAME.into(),
                    network: network.to_string(),
                    extra: None,
                });
            }
            kinds
        };
        let signers = {
            let mut signers = HashMap::with_capacity(1);
            signers.insert(chain_id, self.provider.signer_addresses());
            signers
        };
        Ok(proto::SupportedResponse {
            kinds,
            extensions: Vec::new(),
            signers,
        })
    }
}

/// A fully specified ERC-3009 authorization payload for EVM settlement.
#[derive(Debug)]
pub struct ExactEvmPayment {
    /// Target chain for settlement.
    pub chain: Eip155ChainReference,
    /// Authorized sender (`from`) — EOA or smart wallet.
    pub from: Address,
    /// Authorized recipient (`to`).
    pub to: Address,
    /// Transfer amount (token units).
    pub value: U256,
    /// Not valid before this timestamp (inclusive).
    pub valid_after: UnixTimestamp,
    /// Not valid at/after this timestamp (exclusive).
    pub valid_before: UnixTimestamp,
    /// Unique 32-byte nonce (prevents replay).
    pub nonce: B256,
    /// Raw signature bytes (EIP-1271 or EIP-6492-wrapped).
    pub signature: Bytes,
}

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

/// Runs all preconditions needed for a successful payment:
/// - Valid scheme, network, and receiver.
/// - Valid time window (validAfter/validBefore).
/// - Correct EIP-712 domain construction.
/// - Sufficient on-chain balance.
/// - Sufficient value in payload.
#[instrument(skip_all, err)]
async fn assert_valid_payment<P: Provider>(
    provider: P,
    chain: &Eip155ChainReference,
    payload: &types::PaymentPayload,
    requirements: &types::PaymentRequirements,
) -> Result<(USDC::USDCInstance<P>, ExactEvmPayment, Eip712Domain), FacilitatorLocalError> {
    let payer = payload.payload.authorization.from;
    let chain_id: ChainId = chain.into();
    let payload_chain_id = ChainId::from_network_name(&payload.network)
        .ok_or(FacilitatorLocalError::UnsupportedNetwork(None))?;
    if payload_chain_id != chain_id {
        return Err(FacilitatorLocalError::NetworkMismatch(
            Some(payer.to_string()),
            chain_id.to_string(),
            payload_chain_id.to_string(),
        ));
    }
    let requirements_chain_id = ChainId::from_network_name(&requirements.network)
        .ok_or(FacilitatorLocalError::UnsupportedNetwork(None))?;
    if requirements_chain_id != chain_id {
        return Err(FacilitatorLocalError::NetworkMismatch(
            Some(payer.to_string()),
            chain_id.to_string(),
            requirements_chain_id.to_string(),
        ));
    }
    if payload.scheme != requirements.scheme {
        return Err(FacilitatorLocalError::SchemeMismatch(
            Some(payer.to_string()),
            requirements.scheme.to_string(),
            payload.scheme.to_string(),
        ));
    }
    let authorization = &payload.payload.authorization;
    if authorization.to != requirements.pay_to {
        return Err(FacilitatorLocalError::ReceiverMismatch(
            payer.to_string(),
            authorization.to.to_string(),
            requirements.pay_to.to_string(),
        ));
    }
    let valid_after = authorization.valid_after;
    let valid_before = authorization.valid_before;
    assert_time(payer.into(), valid_after, valid_before)?;
    let asset_address = requirements.asset;
    let contract = USDC::new(asset_address, provider);

    let domain = assert_domain(chain, &contract, &asset_address, requirements).await?;

    let amount_required = requirements.max_amount_required;
    assert_enough_balance(&contract, &authorization.from, amount_required).await?;
    assert_enough_value(&payer, &authorization.value, &amount_required)?;

    let payment = ExactEvmPayment {
        chain: chain.clone(),
        from: authorization.from,
        to: authorization.to,
        value: authorization.value,
        valid_after: authorization.valid_after,
        valid_before: authorization.valid_before,
        nonce: authorization.nonce,
        signature: payload.payload.signature.clone(),
    };

    Ok((contract, payment, domain))
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
    payer: Address,
    valid_after: UnixTimestamp,
    valid_before: UnixTimestamp,
) -> Result<(), FacilitatorLocalError> {
    let now = UnixTimestamp::try_now().map_err(FacilitatorLocalError::ClockError)?;
    if valid_before < now + 6 {
        return Err(FacilitatorLocalError::InvalidTiming(
            payer.to_string(),
            format!("Expired: now {} > valid_before {}", now + 6, valid_before),
        ));
    }
    if valid_after > now {
        return Err(FacilitatorLocalError::InvalidTiming(
            payer.to_string(),
            format!("Not active yet: valid_after {valid_after} > now {now}",),
        ));
    }
    Ok(())
}

/// Constructs the correct EIP-712 domain for signature verification.
///
/// Resolves the `name` and `version` based on:
/// - Static metadata from [`USDCDeployment`] (if available),
/// - Or by calling `version()` on the token contract if not matched statically.
// #[instrument(skip_all, err, fields(
//     network = %payload.network,
//     asset = %asset_address
// ))] FIXME
async fn assert_domain<P: Provider>(
    chain: &Eip155ChainReference,
    token_contract: &USDC::USDCInstance<P>,
    asset_address: &Address,
    requirements: &types::PaymentRequirements,
) -> Result<Eip712Domain, FacilitatorLocalError> {
    let name = requirements.extra.as_ref().map(|extra| extra.name.clone());
    let name = if let Some(name) = name {
        name
    } else {
        token_contract
            .name()
            .call()
            .into_future()
            .instrument(tracing::info_span!(
                "fetch_eip712_name",
                otel.kind = "client",
            ))
            .await
            .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?
    };
    let version = requirements
        .extra
        .as_ref()
        .map(|extra| extra.version.clone());
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
        chain_id: chain.inner(),
        verifying_contract: *asset_address,
    };
    Ok(domain)
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
    sender: &Address,
    max_amount_required: U256,
) -> Result<(), FacilitatorLocalError> {
    let balance = usdc_contract
        .balanceOf(*sender)
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
        Err(FacilitatorLocalError::InsufficientFunds(sender.to_string()))
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
    payer: &Address,
    sent: &U256,
    max_amount_required: &U256,
) -> Result<(), FacilitatorLocalError> {
    if sent < max_amount_required {
        Err(FacilitatorLocalError::InsufficientValue(payer.to_string()))
    } else {
        Ok(())
    }
}

/// Canonical data required to verify a signature.
#[derive(Debug, Clone)]
struct SignedMessage {
    /// Expected signer (an EOA or contract wallet).
    address: Address,
    /// 32-byte digest that was signed (typically an EIP-712 hash).
    hash: B256,
    /// Structured signature, either EIP-6492 or EIP-1271.
    signature: StructuredSignature,
}

sol!(
    /// Solidity-compatible struct definition for ERC-3009 `transferWithAuthorization`.
    ///
    /// This matches the EIP-3009 format used in EIP-712 typed data:
    /// it defines the authorization to transfer tokens from `from` to `to`
    /// for a specific `value`, valid only between `validAfter` and `validBefore`
    /// and identified by a unique `nonce`.
    ///
    /// This struct is primarily used to reconstruct the typed data domain/message
    /// when verifying a client's signature.
    #[derive(Serialize, Deserialize)]
    struct TransferWithAuthorization {
        address from;
        address to;
        uint256 value;
        uint256 validAfter;
        uint256 validBefore;
        bytes32 nonce;
    }
);

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
            validAfter: U256::from(payment.valid_after.as_secs()),
            validBefore: U256::from(payment.valid_before.as_secs()),
            nonce: payment.nonce,
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

impl TryFrom<Bytes> for StructuredSignature {
    type Error = FacilitatorLocalError;

    /// Parse raw signature bytes into a `StructuredSignature`.
    ///
    /// Rules:
    /// - If the last 32 bytes equal [`EIP6492_MAGIC_SUFFIX`], the prefix is
    ///   decoded as a [`Sig6492`] struct and returned as
    ///   [`StructuredSignature::EIP6492`].
    /// - Otherwise, the bytes are returned as [`StructuredSignature::EIP1271`].
    fn try_from(bytes: Bytes) -> Result<Self, Self::Error> {
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
    let from = payment.from;
    let to = payment.to;
    let value = payment.value;
    let valid_after = U256::from(payment.valid_after.as_secs());
    let valid_before = U256::from(payment.valid_before.as_secs());
    let nonce = payment.nonce;
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
    pub from: Address,
    /// The recipient (`to`) address for the authorization.
    pub to: Address,
    /// The amount to transfer (value).
    pub value: U256,
    /// Start of the validity window (inclusive).
    pub valid_after: U256,
    /// End of the validity window (exclusive).
    pub valid_before: U256,
    /// 32-byte authorization nonce (prevents replay).
    pub nonce: B256,
    /// EIP-712 signature for the transfer authorization.
    pub signature: Bytes,
    /// Address of the token contract used for this transfer.
    pub contract_address: Address,
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
