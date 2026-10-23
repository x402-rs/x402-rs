//! Facilitator-side payment verification and settlement for V1 EIP-155 exact scheme.
//!
//! This module implements the facilitator logic for verifying and settling ERC-3009
//! payments on EVM chains. It handles:
//!
//! - Signature verification (EOA, EIP-1271, EIP-6492)
//! - Balance and amount validation
//! - EIP-712 domain construction
//! - On-chain settlement with gas management
//! - Smart wallet deployment for counterfactual signatures

use alloy_contract::SolCallBuilder;
use alloy_primitives::{Address, B256, Bytes, Signature, TxHash, U256, address, hex};
use alloy_provider::bindings::IMulticall3;
use alloy_provider::{
    MULTICALL3_ADDRESS, MulticallError, MulticallItem, PendingTransactionError, Provider,
};
use alloy_rpc_types_eth::TransactionReceipt;
use alloy_sol_types::{Eip712Domain, SolCall, SolStruct, SolType, eip712_domain, sol};
use alloy_transport::TransportError;
use std::collections::HashMap;
use std::fmt::{Debug, Display};
use x402_types::chain::{ChainId, ChainProviderOps};
use x402_types::proto;
use x402_types::proto::{PaymentVerificationError, v1};
use x402_types::scheme::{
    X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError,
};
use x402_types::timestamp::UnixTimestamp;
#[cfg(feature = "telemetry")]
use x402_types::util::telemetry::record_payment_context;

#[cfg(feature = "telemetry")]
use tracing::{Instrument, instrument};
#[cfg(feature = "telemetry")]
use tracing_core::Level;

use crate::V1Eip155Exact;
use crate::chain::{
    EOASignature, EOASignatureExt, Eip155ChainReference, Eip155MetaTransactionProvider,
    Eip3009Authorization, MetaTransaction, MetaTransactionSendError, PendingSettlement,
    pending_settlement_key, settle_request_hash, settlement_of_authorization,
};
use crate::v1_eip155_exact::{
    ExactScheme, PaymentRequirementsExtra, TransferWithAuthorization, types,
};

/// Signature verifier for EIP-6492, EIP-1271, EOA, universally deployed on the supported EVM chains
/// If absent on a target chain, verification will fail; you should deploy the validator there.
// Note: Expect deployed on every chain
pub const VALIDATOR_ADDRESS: Address = address!("0xdAcD51A54883eb67D95FAEb2BBfdC4a9a6BD2a3B");

impl<P> X402SchemeFacilitatorBuilder<P> for V1Eip155Exact
where
    P: Eip155MetaTransactionProvider + ChainProviderOps + Send + Sync + 'static,
    Eip155ExactError: From<P::Error>,
{
    fn build(
        &self,
        provider: P,
        _config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        Ok(Box::new(V1Eip155ExactFacilitator::new(provider)))
    }
}

/// Facilitator for V1 EIP-155 exact scheme payments.
///
/// This struct implements the [`X402SchemeFacilitator`] trait to provide payment
/// verification and settlement services for ERC-3009 based payments on EVM chains.
///
/// # Type Parameters
///
/// - `P`: The provider type, which must implement [`Eip155MetaTransactionProvider`]
///   and [`ChainProviderOps`]
pub struct V1Eip155ExactFacilitator<P> {
    provider: P,
}

impl<P> V1Eip155ExactFacilitator<P> {
    /// Creates a new V1 EIP-155 exact scheme facilitator with the given provider.
    pub fn new(provider: P) -> Self {
        Self { provider }
    }
}

#[async_trait::async_trait]
impl<P> X402SchemeFacilitator for V1Eip155ExactFacilitator<P>
where
    P: Eip155MetaTransactionProvider + ChainProviderOps + Send + Sync,
    P::Inner: Provider,
    Eip155ExactError: From<P::Error>,
{
    #[cfg_attr(feature = "telemetry", instrument(skip_all, err, fields(
        otel.kind = "internal",
        chain_id = tracing::field::Empty,
        payer = tracing::field::Empty,
        pay_to = tracing::field::Empty
    )))]
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError> {
        let request = types::VerifyRequest::try_from(request)?;
        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;
        #[cfg(feature = "telemetry")]
        let chain_id = self.provider.chain_id();
        #[cfg(feature = "telemetry")]
        record_payment_context(
            &chain_id,
            payload.payload.authorization.from,
            requirements.pay_to,
        );
        let (contract, payment, eip712_domain) = assert_valid_payment(
            self.provider.inner(),
            self.provider.chain(),
            payload,
            requirements,
        )
        .await?;

        let payer =
            verify_payment(self.provider.inner(), &contract, &payment, &eip712_domain).await?;

        Ok(v1::VerifyResponse::valid(payer.to_string()).into())
    }

    #[cfg_attr(feature = "telemetry", instrument(skip_all, err, fields(
        otel.kind = "internal",
        chain_id = tracing::field::Empty,
        payer = tracing::field::Empty,
        pay_to = tracing::field::Empty
    )))]
    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, X402SchemeFacilitatorError> {
        let request = types::SettleRequest::try_from(request)?;
        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;
        #[cfg(feature = "telemetry")]
        let chain_id = self.provider.chain_id();
        #[cfg(feature = "telemetry")]
        record_payment_context(
            &chain_id,
            payload.payload.authorization.from,
            requirements.pay_to,
        );
        let authorization =
            authorization_to_reconcile(self.provider.chain(), payload, requirements);
        let tx_hash = settle_with_pending_store(
            &self.provider,
            &payload.payload.signature,
            settle_request_hash(payload, requirements),
            &payload.network,
            || {
                settle_or_find_used_authorization(&self.provider, authorization, async {
                    let (contract, payment, eip712_domain) = assert_valid_payment(
                        self.provider.inner(),
                        self.provider.chain(),
                        payload,
                        requirements,
                    )
                    .await?;
                    settle_payment(&self.provider, &contract, &payment, &eip712_domain).await
                })
            },
        )
        .await?;
        Ok(v1::SettleResponse::Success {
            payer: payload.payload.authorization.from.to_string(),
            transaction: tx_hash.to_string(),
            network: payload.network.clone(),
        }
        .into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError> {
        let chain_id = self.provider.chain_id();
        let kinds = {
            let mut kinds = Vec::with_capacity(1);
            let network = chain_id.as_network_name();
            if let Some(network) = network {
                kinds.push(proto::SupportedPaymentKind {
                    x402_version: v1::X402Version1.into(),
                    scheme: ExactScheme.to_string(),
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

impl ExactEvmPayment {
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
    pub fn as_signed_message(
        &self,
        domain: &Eip712Domain,
    ) -> Result<SignedMessage, StructuredSignatureFormatError> {
        let transfer_with_authorization = TransferWithAuthorization {
            from: self.from,
            to: self.to,
            value: self.value,
            validAfter: U256::from(self.valid_after.as_secs()),
            validBefore: U256::from(self.valid_before.as_secs()),
            nonce: self.nonce,
        };
        let eip712_hash = transfer_with_authorization.eip712_signing_hash(domain);
        let structured_signature: StructuredSignature =
            StructuredSignature::try_from_bytes(self.signature.clone(), self.from, &eip712_hash)?;
        let signed_message = SignedMessage {
            address: self.from,
            hash: eip712_hash,
            signature: structured_signature,
        };
        Ok(signed_message)
    }
}

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    #[sol(rpc)]
    IEIP3009,
    "abi/IEIP3009.json"
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
#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
async fn assert_valid_payment<'a, P: Provider>(
    provider: &'a P,
    chain: &Eip155ChainReference,
    payload: &types::PaymentPayload,
    requirements: &types::PaymentRequirements,
) -> Result<
    (
        IEIP3009::IEIP3009Instance<&'a P>,
        ExactEvmPayment,
        Eip712Domain,
    ),
    Eip155ExactError,
> {
    let chain_id: ChainId = chain.into();
    let payload_chain_id = ChainId::from_network_name(&payload.network)
        .ok_or(PaymentVerificationError::UnsupportedChain)?;
    if payload_chain_id != chain_id {
        return Err(PaymentVerificationError::ChainIdMismatch.into());
    }
    let requirements_chain_id = ChainId::from_network_name(&requirements.network)
        .ok_or(PaymentVerificationError::UnsupportedChain)?;
    if requirements_chain_id != chain_id {
        return Err(PaymentVerificationError::ChainIdMismatch.into());
    }
    let authorization = &payload.payload.authorization;
    if authorization.to != requirements.pay_to {
        return Err(PaymentVerificationError::RecipientMismatch.into());
    }
    let valid_after = authorization.valid_after;
    let valid_before = authorization.valid_before;
    assert_time(valid_after, valid_before)?;
    let asset_address = requirements.asset;
    let contract = IEIP3009::new(asset_address, provider);

    let domain = assert_domain(chain, &contract, &asset_address, &requirements.extra).await?;

    let amount_required = requirements.max_amount_required;
    assert_enough_balance(&contract, &authorization.from, amount_required).await?;
    assert_enough_value(&authorization.value, &amount_required)?;

    let payment = ExactEvmPayment {
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

/// The authorization of a settle request, to look up on chain if settling it fails, when the
/// request matches its requirements on `chain` (networks, recipient and amount). `None` for a
/// request that does not: it fails as before, without a lookup.
fn authorization_to_reconcile(
    chain: &Eip155ChainReference,
    payload: &types::PaymentPayload,
    requirements: &types::PaymentRequirements,
) -> Option<Eip3009Authorization> {
    let chain_id: ChainId = chain.into();
    let authorization = &payload.payload.authorization;
    let matches = ChainId::from_network_name(&payload.network).as_ref() == Some(&chain_id)
        && ChainId::from_network_name(&requirements.network).as_ref() == Some(&chain_id)
        && authorization.to == requirements.pay_to
        && authorization.value >= requirements.max_amount_required;
    matches.then(|| Eip3009Authorization::new(requirements.asset, authorization))
}

/// Validates that the current time is within the `validAfter` and `validBefore` bounds.
///
/// Adds a 6-second grace buffer when checking expiration to account for latency.
#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub fn assert_time(
    valid_after: UnixTimestamp,
    valid_before: UnixTimestamp,
) -> Result<(), PaymentVerificationError> {
    let now = UnixTimestamp::now();
    if valid_before < now + 6 {
        return Err(PaymentVerificationError::Expired);
    }
    if valid_after > now {
        return Err(PaymentVerificationError::Early);
    }
    Ok(())
}

/// Constructs the correct EIP-712 domain for signature verification.
#[cfg_attr(feature = "telemetry", instrument(skip_all, err, fields(
    network = %chain.as_chain_id(),
    asset = %asset_address
)))]
pub async fn assert_domain<P: Provider>(
    chain: &Eip155ChainReference,
    token_contract: &IEIP3009::IEIP3009Instance<P>,
    asset_address: &Address,
    extra: &Option<PaymentRequirementsExtra>,
) -> Result<Eip712Domain, Eip155ExactError> {
    let name = extra.as_ref().map(|extra| extra.name.clone());
    let name = if let Some(name) = name {
        name
    } else {
        let name_b = token_contract.name();
        let name_fut = name_b.call().into_future();
        #[cfg(feature = "telemetry")]
        let name = name_fut
            .instrument(tracing::info_span!(
                "fetch_eip712_name",
                otel.kind = "client",
            ))
            .await?;
        #[cfg(not(feature = "telemetry"))]
        let name = name_fut.await?;
        name
    };
    let version = extra.as_ref().map(|extra| extra.version.clone());
    let version = if let Some(version) = version {
        version
    } else {
        let version_b = token_contract.version();
        let version_fut = version_b.call().into_future();
        #[cfg(feature = "telemetry")]
        let version = version_fut
            .instrument(tracing::info_span!(
                "fetch_eip712_version",
                otel.kind = "client",
            ))
            .await?;
        #[cfg(not(feature = "telemetry"))]
        let version = version_fut.await?;
        version
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
/// Performs an `ERC20.balanceOf()` call using the token contract instance.
#[cfg_attr(feature = "telemetry", instrument(skip_all, err, fields(
    sender = %sender,
    max_required = %max_amount_required,
    token_contract = %ieip3009_token_contract.address()
)))]
pub async fn assert_enough_balance<P: Provider>(
    ieip3009_token_contract: &IEIP3009::IEIP3009Instance<P>,
    sender: &Address,
    max_amount_required: U256,
) -> Result<(), Eip155ExactError> {
    let balance_of = ieip3009_token_contract.balanceOf(*sender);
    let balance_fut = balance_of.call().into_future();
    #[cfg(feature = "telemetry")]
    let balance = balance_fut
        .instrument(tracing::info_span!(
            "fetch_token_balance",
            token_contract = %ieip3009_token_contract.address(),
            sender = %sender,
            otel.kind = "client"
        ))
        .await?;
    #[cfg(not(feature = "telemetry"))]
    let balance = balance_fut.await?;

    if balance < max_amount_required {
        Err(PaymentVerificationError::InsufficientFunds.into())
    } else {
        Ok(())
    }
}

/// Verifies that the declared `value` in the payload is sufficient for the required amount.
///
/// This is a static check (not on-chain) that compares two numbers.
#[cfg_attr(feature = "telemetry", instrument(skip_all, err, fields(
    sent = %sent,
    max_amount_required = %max_amount_required
)))]
pub fn assert_enough_value<V>(
    sent: &V,
    max_amount_required: &V,
) -> Result<(), PaymentVerificationError>
where
    V: Display + Ord,
{
    if sent < max_amount_required {
        Err(PaymentVerificationError::InvalidPaymentAmount)
    } else {
        Ok(())
    }
}

/// Canonical data required to verify a signature.
#[derive(Debug, Clone)]
pub struct SignedMessage {
    /// Expected signer (an EOA or contract wallet).
    address: Address,
    /// 32-byte digest that was signed (typically an EIP-712 hash).
    hash: B256,
    /// Structured signature, either EIP-6492 or EIP-1271 or EOA.
    signature: StructuredSignature,
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
pub enum StructuredSignature {
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
    /// Normalized EOA signature.
    #[allow(clippy::upper_case_acronyms)]
    EOA(EOASignature),
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

#[derive(Debug, thiserror::Error)]
pub enum StructuredSignatureFormatError {
    #[error(transparent)]
    InvalidEIP6492Format(alloy_sol_types::Error),
}

impl StructuredSignature {
    pub fn try_from_bytes(
        bytes: Bytes,
        expected_signer: Address,
        prehash: &B256,
    ) -> Result<Self, StructuredSignatureFormatError> {
        let is_eip6492 = bytes.len() >= 32 && bytes[bytes.len() - 32..] == EIP6492_MAGIC_SUFFIX;
        let signature = if is_eip6492 {
            let body = &bytes[..bytes.len() - 32];
            let sig6492 = Sig6492::abi_decode_params(body)
                .map_err(StructuredSignatureFormatError::InvalidEIP6492Format)?;
            StructuredSignature::EIP6492 {
                factory: sig6492.factory,
                factory_calldata: sig6492.factoryCalldata,
                inner: sig6492.innerSig,
                original: bytes,
            }
        } else {
            // Let's see if it is a EOA signature
            let eoa_signature = if bytes.len() == 65 {
                Signature::from_raw(&bytes).ok().map(|s| s.normalized_s())
            } else if bytes.len() == 64 {
                Some(Signature::from_erc2098(&bytes).normalized_s())
            } else {
                None
            };
            match eoa_signature {
                None => StructuredSignature::EIP1271(bytes),
                Some(s) => {
                    let is_expected_signer = s
                        .recover_address_from_prehash(prehash)
                        .ok()
                        .map(|r| r == expected_signer)
                        .unwrap_or(false);
                    if is_expected_signer {
                        StructuredSignature::EOA(EOASignature::new(s))
                    } else {
                        StructuredSignature::EIP1271(bytes)
                    }
                }
            }
        };
        Ok(signature)
    }
}

impl From<StructuredSignature> for Bytes {
    fn from(value: StructuredSignature) -> Self {
        match value {
            StructuredSignature::EIP6492 { inner, .. } => inner,
            StructuredSignature::EOA(sig) => Bytes::from(sig.as_ref().as_bytes()),
            StructuredSignature::EIP1271(sig) => sig,
        }
    }
}

impl TryFrom<Bytes> for StructuredSignature {
    type Error = StructuredSignatureFormatError;

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
            let sig6492 = Sig6492::abi_decode_params(body)
                .map_err(StructuredSignatureFormatError::InvalidEIP6492Format)?;
            StructuredSignature::EIP6492 {
                factory: sig6492.factory,
                factory_calldata: sig6492.factoryCalldata,
                inner: sig6492.innerSig,
                original: bytes,
            }
        } else {
            StructuredSignature::EIP1271(bytes)
        };
        Ok(signature)
    }
}

pub struct TransferWithAuthorization0Call<P>(
    pub TransferWithAuthorizationCall<P, IEIP3009::transferWithAuthorization_0Call, Bytes>,
);

impl<'a, P: Provider> TransferWithAuthorization0Call<&'a P> {
    /// Constructs a full `transferWithAuthorization` call for a verified payment payload.
    ///
    /// This function prepares the transaction builder with gas pricing adapted to the network's
    /// capabilities (EIP-1559 or legacy) and packages it together with signature metadata
    /// into a [`TransferWithAuthorization0Call`] structure.
    ///
    /// This function does not perform any validation — it assumes inputs are already checked.
    pub fn new(
        contract: &'a IEIP3009::IEIP3009Instance<P>,
        payment: &ExactEvmPayment,
        signature: Bytes,
    ) -> Self {
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
        TransferWithAuthorization0Call(TransferWithAuthorizationCall {
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
}

pub struct TransferWithAuthorization1Call<P>(
    pub TransferWithAuthorizationCall<P, IEIP3009::transferWithAuthorization_1Call, Signature>,
);

impl<'a, P: Provider> TransferWithAuthorization1Call<&'a P> {
    /// Constructs a full `transferWithAuthorization` call for a verified payment payload
    /// using split signature components (v, r, s).
    ///
    /// This function prepares the transaction builder with gas pricing adapted to the network's
    /// capabilities (EIP-1559 or legacy) and packages it together with signature metadata
    /// into a [`TransferWithAuthorization1Call`] structure.
    ///
    /// This function does not perform any validation — it assumes inputs are already checked.
    pub fn new(
        contract: &'a IEIP3009::IEIP3009Instance<P>,
        payment: &ExactEvmPayment,
        signature: Signature,
    ) -> Self {
        let from = payment.from;
        let to = payment.to;
        let value = payment.value;
        let valid_after = U256::from(payment.valid_after.as_secs());
        let valid_before = U256::from(payment.valid_before.as_secs());
        let nonce = payment.nonce;
        let v = signature.v_legacy();
        let r = signature.r_bytes();
        let s = signature.s_bytes();
        let tx = contract.transferWithAuthorization_1(
            from,
            to,
            value,
            valid_after,
            valid_before,
            nonce,
            v,
            r,
            s,
        );
        TransferWithAuthorization1Call(TransferWithAuthorizationCall {
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
}

/// A prepared call to `transferWithAuthorization` (ERC-3009) including all derived fields.
///
/// This struct wraps the assembled call builder, making it reusable across verification
/// (`.call()`) and settlement (`.send()`) flows, along with context useful for tracing/logging.
pub struct TransferWithAuthorizationCall<P, TCall, TSignature> {
    /// The prepared call builder that can be `.call()`ed or `.send()`ed.
    pub tx: SolCallBuilder<P, TCall>,
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
    pub signature: TSignature,
    /// Address of the token contract used for this transfer.
    pub contract_address: Address,
}

/// Check whether contract code is present at `address`.
///
/// Uses `eth_getCode` against this provider. This is useful after a counterfactual
/// deployment to confirm visibility on the sending RPC before submitting a
/// follow-up transaction.
pub async fn is_contract_deployed<P: Provider>(
    provider: &P,
    address: &Address,
) -> Result<bool, TransportError> {
    let bytes_fut = provider.get_code_at(*address).into_future();
    #[cfg(feature = "telemetry")]
    let bytes = bytes_fut
        .instrument(tracing::info_span!("get_code_at",
            address = %address,
            otel.kind = "client",
        ))
        .await?;
    #[cfg(not(feature = "telemetry"))]
    let bytes = bytes_fut.await?;
    Ok(!bytes.is_empty())
}

pub async fn verify_payment<P: Provider>(
    provider: &P,
    contract: &IEIP3009::IEIP3009Instance<&P>,
    payment: &ExactEvmPayment,
    eip712_domain: &Eip712Domain,
) -> Result<Address, Eip155ExactError> {
    let signed_message = payment.as_signed_message(eip712_domain)?;

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
            let validator6492 = Validator6492::new(VALIDATOR_ADDRESS, &provider);
            let is_valid_signature_call =
                validator6492.isValidSigWithSideEffects(payer, hash, original);
            // Prepare the call to simulate transfer the funds
            let transfer_call = TransferWithAuthorization0Call::new(contract, payment, inner);
            let transfer_call = transfer_call.0;
            // Execute both calls in a single transaction simulation to accommodate for possible smart wallet creation
            let aggregate3 = provider
                .multicall()
                .add(is_valid_signature_call)
                .add(transfer_call.tx);
            let aggregate3_call = aggregate3.aggregate3();
            #[cfg(feature = "telemetry")]
            let (is_valid_signature_result, transfer_result) = aggregate3_call
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
                .await?;
            #[cfg(not(feature = "telemetry"))]
            let (is_valid_signature_result, transfer_result) = aggregate3_call.await?;
            let is_valid_signature_result = is_valid_signature_result
                .map_err(|e| PaymentVerificationError::InvalidSignature(e.to_string()))?;
            if !is_valid_signature_result {
                return Err(PaymentVerificationError::InvalidSignature(
                    "Chain reported signature to be invalid".to_string(),
                )
                .into());
            }
            transfer_result
                .map_err(|e| PaymentVerificationError::TransactionSimulation(e.to_string()))?;
        }
        StructuredSignature::EIP1271(signature) => {
            // It is EIP-1271 signature, which we can pass to the transfer simulation
            let transfer_call = TransferWithAuthorization0Call::new(contract, payment, signature);
            let transfer_call = transfer_call.0;
            let transfer_call_fut = transfer_call.tx.call().into_future();
            #[cfg(feature = "telemetry")]
            transfer_call_fut
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
                .await?;
            #[cfg(not(feature = "telemetry"))]
            transfer_call_fut.await?;
        }
        StructuredSignature::EOA(signature) => {
            // It is EOA signature, which we can pass to the transfer simulation of (r,s,v)-based transferWithAuthorization function
            let transfer_call =
                TransferWithAuthorization1Call::new(contract, payment, signature.into());
            let transfer_call = transfer_call.0;
            let transfer_call_fut = transfer_call.tx.call().into_future();
            #[cfg(feature = "telemetry")]
            transfer_call_fut
                .instrument(tracing::info_span!("call_transferWithAuthorization_1",
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
                .await?;
            #[cfg(not(feature = "telemetry"))]
            transfer_call_fut.await?;
        }
    }

    Ok(payer)
}

pub async fn settle_payment<P, E>(
    provider: &P,
    contract: &IEIP3009::IEIP3009Instance<&P::Inner>,
    payment: &ExactEvmPayment,
    eip712_domain: &Eip712Domain,
) -> Result<TxHash, Eip155ExactError>
where
    P: Eip155MetaTransactionProvider<Error = E>,
    Eip155ExactError: From<E>,
{
    let signed_message = payment.as_signed_message(eip712_domain)?;
    let payer = payment.from;
    let receipt = match signed_message.signature {
        StructuredSignature::EIP6492 {
            factory,
            factory_calldata,
            inner,
            original: _,
        } => {
            let is_contract_deployed = is_contract_deployed(provider.inner(), &payer).await?;
            let transfer_call = TransferWithAuthorization0Call::new(contract, payment, inner);
            let transfer_call = transfer_call.0;
            if is_contract_deployed {
                // transferWithAuthorization with inner signature
                let meta_tx = MetaTransaction::new(
                    transfer_call.tx.target(),
                    transfer_call.tx.calldata().clone(),
                );
                let tx_fut = Eip155MetaTransactionProvider::send_transaction(provider, meta_tx);
                #[cfg(feature = "telemetry")]
                let receipt = tx_fut
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
                    .await?;
                #[cfg(not(feature = "telemetry"))]
                let receipt = tx_fut.await?;
                receipt
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
                let meta_tx =
                    MetaTransaction::new(MULTICALL3_ADDRESS, aggregate_call.abi_encode().into());
                let tx_fut = Eip155MetaTransactionProvider::send_transaction(provider, meta_tx);
                #[cfg(feature = "telemetry")]
                let receipt = tx_fut
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
                    .await?;
                #[cfg(not(feature = "telemetry"))]
                let receipt = tx_fut.await?;
                receipt
            }
        }
        StructuredSignature::EIP1271(eip1271_signature) => {
            let transfer_call =
                TransferWithAuthorization0Call::new(contract, payment, eip1271_signature);
            let transfer_call = transfer_call.0;
            // transferWithAuthorization with eip1271 signature
            let meta_tx = MetaTransaction::new(
                transfer_call.tx.target(),
                transfer_call.tx.calldata().clone(),
            );
            let tx_fut = Eip155MetaTransactionProvider::send_transaction(provider, meta_tx);
            #[cfg(feature = "telemetry")]
            let receipt = tx_fut
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
                .await?;
            #[cfg(not(feature = "telemetry"))]
            let receipt = tx_fut.await?;
            receipt
        }
        StructuredSignature::EOA(signature) => {
            let transfer_call =
                TransferWithAuthorization1Call::new(contract, payment, signature.into());
            let transfer_call = transfer_call.0;
            // transferWithAuthorization with EOA signature
            let meta_tx = MetaTransaction::new(
                transfer_call.tx.target(),
                transfer_call.tx.calldata().clone(),
            );
            let tx_fut = Eip155MetaTransactionProvider::send_transaction(provider, meta_tx);
            #[cfg(feature = "telemetry")]
            let receipt = tx_fut
                .instrument(tracing::info_span!("call_transferWithAuthorization_1",
                    from = %transfer_call.from,
                    to = %transfer_call.to,
                    value = %transfer_call.value,
                    valid_after = %transfer_call.valid_after,
                    valid_before = %transfer_call.valid_before,
                    nonce = %transfer_call.nonce,
                    signature = %transfer_call.signature,
                    token_contract = %transfer_call.contract_address,
                    sig_kind="EOA",
                    otel.kind = "client",
                ))
                .await?;
            #[cfg(not(feature = "telemetry"))]
            let receipt = tx_fut.await?;
            receipt
        }
    };
    tx_hash_from_receipt(&receipt)
}

/// Settles a payment through the provider's pending-settlement store, if it keeps one.
///
/// `signature` identifies the payment and `request_hash` the settle request (see
/// [`settle_request_hash`]). A request that already answered `settlement_pending` is not
/// verified or sent again: it is reconciled against the transaction it broadcast, and answers
/// success once that receipt is found, a failure if the transaction reverted, or
/// `settlement_pending` with the same hash if the receipt is still unavailable. Any other request
/// runs `settle`; if that ends with the receipt unavailable, the hash is recorded so the
/// resource server's retry can be reconciled.
pub async fn settle_with_pending_store<P, F, Fut>(
    provider: &P,
    signature: &[u8],
    request_hash: Option<B256>,
    network: &str,
    settle: F,
) -> Result<TxHash, X402SchemeFacilitatorError>
where
    P: Eip155MetaTransactionProvider,
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<TxHash, Eip155ExactError>>,
{
    let (Some(store), Some(request_hash)) = (provider.pending_settlements(), request_hash) else {
        return settle().await.map_err(|e| e.into_settlement_error(network));
    };
    let key = pending_settlement_key(signature);
    // Taken before reconciling, so a concurrent copy of the same request does not reconcile too:
    // it settles from scratch and fails on the used authorization, unless the provider answers
    // used authorizations from the chain (see `settle_or_find_used_authorization`).
    if let Some(tx_hash) = store.take(&key, &request_hash).await {
        #[cfg(feature = "telemetry")]
        tracing::info!(tx = %tx_hash, "Retried settlement: reconciling with the transaction already broadcast");
        return match provider.find_receipt(tx_hash).await {
            Some(receipt) => {
                tx_hash_from_receipt(&receipt).map_err(|e| e.into_settlement_error(network))
            }
            None => {
                store
                    .insert(
                        key,
                        PendingSettlement {
                            tx_hash,
                            request_hash,
                        },
                    )
                    .await;
                Err(X402SchemeFacilitatorError::SettlementPending {
                    network: network.to_string(),
                    transaction: tx_hash.to_string(),
                    details: format!(
                        "Transaction {tx_hash} was broadcast for this payment by an earlier request; its receipt is still unavailable"
                    ),
                })
            }
        };
    }
    let result = settle().await;
    if let Err(Eip155ExactError::ReceiptUnavailable { tx_hash, .. }) = &result {
        store
            .insert(
                key,
                PendingSettlement {
                    tx_hash: *tx_hash,
                    request_hash,
                },
            )
            .await;
    }
    result.map_err(|e| e.into_settlement_error(network))
}

/// Runs `settle` and, if it fails because the request's ERC-3009 authorization was already
/// used, answers with the transaction that used it.
///
/// This is for a settle request that has no pending-settlement entry: the retry of a
/// settlement whose entry was kept by another instance, or lost in a restart. `authorization`
/// is the request's authorization (see `authorization_to_reconcile`), or `None` for no lookup.
/// When `settle` fails with any error but [`Eip155ExactError::ReceiptUnavailable`] (which
/// leaves a pending settlement of its own), the provider looks the authorization's use up
/// ([`Eip155MetaTransactionProvider::find_authorization_use`], which looks nothing up unless
/// the provider enables it). The transaction found is the request's settlement if it
/// succeeded, was sent by one of the provider's signers, and made the asset emit
/// `AuthorizationUsed` for the authorization and a `Transfer` of its value from the payer to the
/// recipient ([`settlement_of_authorization`]): its hash is then the answer. If its receipt
/// could not be fetched, the answer is
/// [`Eip155ExactError::AuthorizationUseReceiptUnavailable`] with its hash, which `/settle`
/// answers as `settlement_pending` but which is not recorded in the pending-settlement store,
/// so a later retry is checked again. Otherwise, as when nothing is found, the error of
/// `settle` is.
///
/// Permit2 payments are not covered: their nonces are in the Permit2 contract.
pub async fn settle_or_find_used_authorization<P, Fut>(
    provider: &P,
    authorization: Option<Eip3009Authorization>,
    settle: Fut,
) -> Result<TxHash, Eip155ExactError>
where
    P: Eip155MetaTransactionProvider + ChainProviderOps,
    Fut: Future<Output = Result<TxHash, Eip155ExactError>>,
{
    let error = match settle.await {
        Err(error) if !matches!(error, Eip155ExactError::ReceiptUnavailable { .. }) => error,
        result => return result,
    };
    let Some(authorization) = authorization else {
        return Err(error);
    };
    let Some(found) = provider.find_authorization_use(authorization).await else {
        return Err(error);
    };
    let Some(receipt) = found.receipt else {
        #[cfg(feature = "telemetry")]
        tracing::info!(tx = %found.tx_hash, nonce = %authorization.nonce, "The authorization was used by a transaction whose receipt is unavailable");
        return Err(Eip155ExactError::AuthorizationUseReceiptUnavailable(
            found.tx_hash,
        ));
    };
    let signers = provider
        .signer_addresses()
        .iter()
        .filter_map(|signer| signer.parse().ok())
        .collect::<Vec<Address>>();
    match settlement_of_authorization(&receipt, &authorization, &signers) {
        Ok(tx_hash) => {
            #[cfg(feature = "telemetry")]
            tracing::info!(tx = %tx_hash, nonce = %authorization.nonce, "Settle request answered with the transaction that used its authorization");
            Ok(tx_hash)
        }
        Err(mismatch) => {
            #[cfg(feature = "telemetry")]
            tracing::warn!(tx = %receipt.transaction_hash, nonce = %authorization.nonce, %mismatch, "Transaction that used the authorization is not this request's settlement");
            let _ = mismatch;
            Err(error)
        }
    }
}

/// Extracts the transaction hash from a confirmed receipt, or returns an error if the
/// transaction reverted.
///
/// Alloy's [`TransactionReceipt`] always contains a hash, but a receipt with
/// `status = false` means the EVM execution reverted (e.g. the token transfer was
/// rejected). This helper centralises that check so callers can treat a reverted
/// receipt as an [`Eip155ExactError::TransactionReverted`] rather than silently
/// returning the hash of a failed transaction.
pub fn tx_hash_from_receipt(receipt: &TransactionReceipt) -> Result<TxHash, Eip155ExactError> {
    let success = receipt.status();
    if success {
        #[cfg(feature = "telemetry")]
        tracing::event!(Level::INFO,
            status = "ok",
            tx = %receipt.transaction_hash,
            "transfer succeeded"
        );
        Ok(receipt.transaction_hash)
    } else {
        #[cfg(feature = "telemetry")]
        tracing::event!(
            Level::WARN,
            status = "failed",
            tx = %receipt.transaction_hash,
            "transfer failed"
        );
        Err(Eip155ExactError::TransactionReverted(
            receipt.transaction_hash,
        ))
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Eip155ExactError {
    #[error(transparent)]
    Transport(#[from] TransportError),
    #[error(transparent)]
    PendingTransaction(#[from] PendingTransactionError),
    /// The transaction was broadcast, but its receipt could not be fetched; it may still be mined.
    #[error("Transaction {tx_hash} was broadcast, but its receipt could not be fetched: {source}")]
    ReceiptUnavailable {
        tx_hash: TxHash,
        #[source]
        source: PendingTransactionError,
    },
    /// A settle request with no pending-settlement entry found the transaction that used its
    /// authorization, but not its receipt, so whether it is the request's settlement is not
    /// known yet.
    #[error("The authorization was used by transaction {0}, whose receipt could not be fetched")]
    AuthorizationUseReceiptUnavailable(TxHash),
    #[error("Transaction {0} reverted")]
    TransactionReverted(TxHash),
    #[error("Contract call failed: {0}")]
    ContractCall(String),
    #[error(transparent)]
    PaymentVerification(#[from] PaymentVerificationError),
}

impl Eip155ExactError {
    /// Converts an error from a settlement on `network` into the error `/settle` answers with.
    ///
    /// [`Eip155ExactError::ReceiptUnavailable`] and
    /// [`Eip155ExactError::AuthorizationUseReceiptUnavailable`] become
    /// [`X402SchemeFacilitatorError::SettlementPending`], which keeps the transaction hash and
    /// the network so the caller can reconcile on chain. Every other error converts as
    /// [`From`] does.
    pub fn into_settlement_error(self, network: &str) -> X402SchemeFacilitatorError {
        if let Eip155ExactError::ReceiptUnavailable { tx_hash, .. }
        | Eip155ExactError::AuthorizationUseReceiptUnavailable(tx_hash) = &self
        {
            return X402SchemeFacilitatorError::SettlementPending {
                network: network.to_string(),
                transaction: tx_hash.to_string(),
                details: self.to_string(),
            };
        }
        self.into()
    }
}

impl From<Eip155ExactError> for X402SchemeFacilitatorError {
    fn from(value: Eip155ExactError) -> Self {
        match value {
            Eip155ExactError::Transport(_) => Self::OnchainFailure(value.to_string()),
            Eip155ExactError::PendingTransaction(_) => Self::OnchainFailure(value.to_string()),
            // Without the network a pending answer is incomplete; settlement code paths use
            // `into_settlement_error` instead. The message still names the transaction.
            Eip155ExactError::ReceiptUnavailable { .. } => Self::OnchainFailure(value.to_string()),
            Eip155ExactError::AuthorizationUseReceiptUnavailable(_) => {
                Self::OnchainFailure(value.to_string())
            }
            Eip155ExactError::TransactionReverted(_) => Self::OnchainFailure(value.to_string()),
            Eip155ExactError::ContractCall(_) => Self::OnchainFailure(value.to_string()),
            Eip155ExactError::PaymentVerification(e) => Self::PaymentVerification(e),
        }
    }
}

impl From<StructuredSignatureFormatError> for Eip155ExactError {
    fn from(e: StructuredSignatureFormatError) -> Self {
        Self::PaymentVerification(PaymentVerificationError::InvalidSignature(e.to_string()))
    }
}

impl From<MetaTransactionSendError> for Eip155ExactError {
    fn from(e: MetaTransactionSendError) -> Self {
        match e {
            MetaTransactionSendError::Transport(e) => Self::Transport(e),
            MetaTransactionSendError::PendingTransaction(e) => Self::PendingTransaction(e),
            MetaTransactionSendError::ReceiptUnavailable { tx_hash, source } => {
                Self::ReceiptUnavailable { tx_hash, source }
            }
            MetaTransactionSendError::Custom(e) => Self::ContractCall(e),
        }
    }
}

impl From<MulticallError> for Eip155ExactError {
    fn from(e: MulticallError) -> Self {
        match e {
            MulticallError::ValueTx => Self::PaymentVerification(
                PaymentVerificationError::TransactionSimulation(e.to_string()),
            ),
            MulticallError::DecodeError(_) => Self::PaymentVerification(
                PaymentVerificationError::TransactionSimulation(e.to_string()),
            ),
            MulticallError::NoReturnData => Self::PaymentVerification(
                PaymentVerificationError::TransactionSimulation(e.to_string()),
            ),
            MulticallError::CallFailed(_) => Self::PaymentVerification(
                PaymentVerificationError::TransactionSimulation(e.to_string()),
            ),
            MulticallError::TransportError(transport_error) => Self::Transport(transport_error),
        }
    }
}

impl From<alloy_contract::Error> for Eip155ExactError {
    fn from(e: alloy_contract::Error) -> Self {
        match e {
            alloy_contract::Error::UnknownFunction(_) => Self::ContractCall(e.to_string()),
            alloy_contract::Error::UnknownSelector(_) => Self::ContractCall(e.to_string()),
            alloy_contract::Error::NotADeploymentTransaction => Self::ContractCall(e.to_string()),
            alloy_contract::Error::ContractNotDeployed => Self::ContractCall(e.to_string()),
            alloy_contract::Error::ZeroData(_, _) => Self::ContractCall(e.to_string()),
            alloy_contract::Error::AbiError(_) => Self::ContractCall(e.to_string()),
            alloy_contract::Error::TransportError(e) => Self::Transport(e),
            alloy_contract::Error::PendingTransactionError(e) => Self::PendingTransaction(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::b256;
    use alloy_provider::WatchTxError;
    use alloy_transport::TransportErrorKind;
    use x402_types::proto::{AsPaymentProblem, ErrorReason};

    const TX_HASH: TxHash =
        b256!("0xa5f6a7027df46b5da388406fbc39e468f2466968f698ffbd21ad0e674af58601");

    /// The error an RPC behind a load balancer with no healthy backend answers with.
    fn rpc_503() -> TransportError {
        TransportErrorKind::http_error(
            503,
            r#"{"jsonrpc":"2.0","error":{"code":-32011,"message":"no backend is currently healthy to serve traffic"},"id":1}"#
                .to_string(),
        )
    }

    #[test]
    fn test_receipt_error_after_broadcast_settles_as_pending_with_hash() {
        let send_error = MetaTransactionSendError::ReceiptUnavailable {
            tx_hash: TX_HASH,
            source: PendingTransactionError::TransportError(rpc_503()),
        };
        let error = Eip155ExactError::from(send_error).into_settlement_error("celo");
        let X402SchemeFacilitatorError::SettlementPending {
            network,
            transaction,
            details,
        } = &error
        else {
            panic!("expected SettlementPending, got {error:?}");
        };
        assert_eq!(network, "celo");
        assert_eq!(transaction, &TX_HASH.to_string());
        assert!(details.contains("HTTP error 503"));
        assert_eq!(
            error.as_payment_problem().reason(),
            ErrorReason::SettlementPending
        );
    }

    #[test]
    fn test_receipt_timeout_after_broadcast_settles_as_pending_with_hash() {
        let send_error = MetaTransactionSendError::ReceiptUnavailable {
            tx_hash: TX_HASH,
            source: PendingTransactionError::TxWatcher(WatchTxError::Timeout),
        };
        let error = Eip155ExactError::from(send_error).into_settlement_error("eip155:42220");
        assert!(matches!(
            &error,
            X402SchemeFacilitatorError::SettlementPending { network, transaction, .. }
                if network == "eip155:42220" && transaction == &TX_HASH.to_string()
        ));
    }

    #[test]
    fn test_send_error_before_broadcast_stays_a_failure() {
        let send_error = MetaTransactionSendError::Transport(rpc_503());
        let error = Eip155ExactError::from(send_error).into_settlement_error("celo");
        assert!(matches!(
            error,
            X402SchemeFacilitatorError::OnchainFailure(_)
        ));
        assert_eq!(
            error.as_payment_problem().reason(),
            ErrorReason::UnexpectedError
        );
    }

    #[test]
    fn test_reverted_transaction_stays_a_failure() {
        let error = Eip155ExactError::TransactionReverted(TX_HASH).into_settlement_error("celo");
        assert!(matches!(
            error,
            X402SchemeFacilitatorError::OnchainFailure(_)
        ));
    }

    mod pending_store {
        use super::*;
        use crate::chain::{
            AuthorizationUse, InMemoryPendingSettlementStore, PendingSettlementStore,
        };
        use alloy_primitives::address;
        use alloy_provider::RootProvider;
        use alloy_sol_types::SolEvent;
        use std::sync::Arc;
        use std::sync::Mutex;
        use std::sync::atomic::{AtomicU32, Ordering};

        const SIGNATURE: &[u8] = b"signature of the payment";
        const REQUEST: B256 =
            b256!("0x2222222222222222222222222222222222222222222222222222222222222222");
        const OTHER_REQUEST: B256 =
            b256!("0x3333333333333333333333333333333333333333333333333333333333333333");
        const OTHER_TX: TxHash =
            b256!("0x4444444444444444444444444444444444444444444444444444444444444444");

        /// A provider whose receipt and authorization-use lookups answer what the test sets,
        /// with an optional store and one signer.
        #[derive(Debug)]
        struct MockProvider {
            store: Option<InMemoryPendingSettlementStore>,
            receipt: Mutex<Option<TransactionReceipt>>,
            receipt_lookups: AtomicU32,
            authorization_use: Mutex<Option<AuthorizationUse>>,
            authorization_lookups: Mutex<Vec<Eip3009Authorization>>,
            inner: RootProvider,
            chain: Eip155ChainReference,
        }

        impl MockProvider {
            fn new(store: bool) -> Self {
                Self {
                    store: store.then(InMemoryPendingSettlementStore::default),
                    receipt: Mutex::new(None),
                    receipt_lookups: AtomicU32::new(0),
                    authorization_use: Mutex::new(None),
                    authorization_lookups: Mutex::new(Vec::new()),
                    inner: RootProvider::new_http("http://127.0.0.1:9".parse().unwrap()),
                    chain: Eip155ChainReference::new(84532),
                }
            }

            fn set_authorization_use(&self, receipt: Option<TransactionReceipt>) {
                *self.authorization_use.lock().unwrap() = receipt.map(|receipt| AuthorizationUse {
                    tx_hash: receipt.transaction_hash,
                    receipt: Some(receipt),
                });
            }

            fn set_authorization_use_without_receipt(&self, tx_hash: TxHash) {
                *self.authorization_use.lock().unwrap() = Some(AuthorizationUse {
                    tx_hash,
                    receipt: None,
                });
            }

            fn authorization_lookups(&self) -> Vec<Eip3009Authorization> {
                self.authorization_lookups.lock().unwrap().clone()
            }

            fn set_receipt(&self, receipt: Option<TransactionReceipt>) {
                *self.receipt.lock().unwrap() = receipt;
            }

            fn stored(&self) -> usize {
                self.store.as_ref().map_or(0, |s| s.len())
            }
        }

        impl Eip155MetaTransactionProvider for MockProvider {
            type Error = MetaTransactionSendError;
            type Inner = RootProvider;

            fn inner(&self) -> &Self::Inner {
                &self.inner
            }

            fn chain(&self) -> &Eip155ChainReference {
                &self.chain
            }

            async fn send_transaction(
                &self,
                _tx: MetaTransaction,
            ) -> Result<TransactionReceipt, Self::Error> {
                unreachable!("settlement in these tests goes through the closure")
            }

            fn pending_settlements(&self) -> Option<&dyn PendingSettlementStore> {
                self.store
                    .as_ref()
                    .map(|s| s as &dyn PendingSettlementStore)
            }

            async fn find_receipt(&self, _tx_hash: TxHash) -> Option<TransactionReceipt> {
                self.receipt_lookups.fetch_add(1, Ordering::Relaxed);
                self.receipt.lock().unwrap().clone()
            }

            async fn find_authorization_use(
                &self,
                authorization: Eip3009Authorization,
            ) -> Option<AuthorizationUse> {
                self.authorization_lookups
                    .lock()
                    .unwrap()
                    .push(authorization);
                self.authorization_use.lock().unwrap().clone()
            }
        }

        impl ChainProviderOps for MockProvider {
            fn signer_addresses(&self) -> Vec<String> {
                vec![SIGNER.to_string()]
            }

            fn chain_id(&self) -> ChainId {
                self.chain.into()
            }
        }

        fn receipt(tx_hash: TxHash, success: bool) -> TransactionReceipt {
            serde_json::from_value(serde_json::json!({
                "type": "0x2",
                "status": if success { "0x1" } else { "0x0" },
                "cumulativeGasUsed": "0x1",
                "logs": [],
                "logsBloom": format!("0x{}", "0".repeat(512)),
                "transactionHash": tx_hash,
                "transactionIndex": "0x0",
                "blockHash": "0x1111111111111111111111111111111111111111111111111111111111111111",
                "blockNumber": "0x10",
                "gasUsed": "0x1",
                "effectiveGasPrice": "0x1",
                "from": "0x0000000000000000000000000000000000000001",
                "to": "0x0000000000000000000000000000000000000002",
                "contractAddress": null
            }))
            .unwrap()
        }

        fn receipt_unavailable() -> Eip155ExactError {
            Eip155ExactError::ReceiptUnavailable {
                tx_hash: TX_HASH,
                source: PendingTransactionError::TransportError(rpc_503()),
            }
        }

        /// Settles through the store; `outcome` is what a settlement from scratch would give.
        async fn settle(
            provider: &impl Eip155MetaTransactionProvider,
            request: B256,
            sends: &AtomicU32,
            outcome: impl FnOnce() -> Result<TxHash, Eip155ExactError>,
        ) -> Result<TxHash, X402SchemeFacilitatorError> {
            settle_with_pending_store(provider, SIGNATURE, Some(request), "eip155:84532", || {
                sends.fetch_add(1, Ordering::Relaxed);
                async move { outcome() }
            })
            .await
        }

        fn assert_pending_with(error: &X402SchemeFacilitatorError, tx_hash: TxHash) {
            assert!(
                matches!(
                    error,
                    X402SchemeFacilitatorError::SettlementPending { network, transaction, .. }
                        if network == "eip155:84532" && transaction == &tx_hash.to_string()
                ),
                "expected settlement_pending with {tx_hash}, got {error:?}"
            );
        }

        #[tokio::test]
        async fn test_receipt_unavailable_is_recorded_for_the_retry() {
            let provider = MockProvider::new(true);
            let sends = AtomicU32::new(0);
            let error = settle(&provider, REQUEST, &sends, || Err(receipt_unavailable()))
                .await
                .unwrap_err();
            assert_pending_with(&error, TX_HASH);
            assert_eq!(provider.stored(), 1);
            assert_eq!(sends.load(Ordering::Relaxed), 1);
        }

        #[tokio::test]
        async fn test_retry_reconciles_with_the_broadcast_transaction_without_sending_again() {
            let provider = MockProvider::new(true);
            let sends = AtomicU32::new(0);
            let _ = settle(&provider, REQUEST, &sends, || Err(receipt_unavailable())).await;
            provider.set_receipt(Some(receipt(TX_HASH, true)));
            let tx_hash = settle(&provider, REQUEST, &sends, || {
                panic!("a reconciled retry must not settle again")
            })
            .await
            .unwrap();
            assert_eq!(tx_hash, TX_HASH);
            assert_eq!(sends.load(Ordering::Relaxed), 1);
            assert_eq!(provider.receipt_lookups.load(Ordering::Relaxed), 1);
            assert_eq!(provider.stored(), 0);
        }

        #[tokio::test]
        async fn test_retry_while_the_receipt_is_still_unavailable_stays_pending_with_the_same_hash()
         {
            let provider = MockProvider::new(true);
            let sends = AtomicU32::new(0);
            let _ = settle(&provider, REQUEST, &sends, || Err(receipt_unavailable())).await;
            let error = settle(&provider, REQUEST, &sends, || {
                panic!("a reconciled retry must not settle again")
            })
            .await
            .unwrap_err();
            assert_pending_with(&error, TX_HASH);
            assert_eq!(provider.stored(), 1, "kept for a later retry");
            // And a later retry still reconciles once the receipt shows up.
            provider.set_receipt(Some(receipt(TX_HASH, true)));
            let tx_hash = settle(&provider, REQUEST, &sends, || {
                panic!("a reconciled retry must not settle again")
            })
            .await
            .unwrap();
            assert_eq!(tx_hash, TX_HASH);
            assert_eq!(sends.load(Ordering::Relaxed), 1);
        }

        #[tokio::test]
        async fn test_retry_of_a_reverted_settlement_is_a_failure() {
            let provider = MockProvider::new(true);
            let sends = AtomicU32::new(0);
            let _ = settle(&provider, REQUEST, &sends, || Err(receipt_unavailable())).await;
            provider.set_receipt(Some(receipt(TX_HASH, false)));
            let error = settle(&provider, REQUEST, &sends, || {
                panic!("a reconciled retry must not settle again")
            })
            .await
            .unwrap_err();
            assert!(matches!(
                error,
                X402SchemeFacilitatorError::OnchainFailure(_)
            ));
            assert_eq!(provider.stored(), 0);
        }

        #[tokio::test]
        async fn test_same_signature_in_another_request_settles_from_scratch() {
            let provider = MockProvider::new(true);
            let sends = AtomicU32::new(0);
            let _ = settle(&provider, REQUEST, &sends, || Err(receipt_unavailable())).await;
            provider.set_receipt(Some(receipt(TX_HASH, true)));
            // Settled from scratch, which on chain fails because the authorization is used.
            let error = settle(&provider, OTHER_REQUEST, &sends, || {
                Err(Eip155ExactError::ContractCall(
                    "FiatTokenV2: authorization is used or canceled".to_string(),
                ))
            })
            .await
            .unwrap_err();
            assert!(matches!(
                error,
                X402SchemeFacilitatorError::OnchainFailure(_)
            ));
            assert_eq!(sends.load(Ordering::Relaxed), 2);
            assert_eq!(provider.receipt_lookups.load(Ordering::Relaxed), 0);
            assert_eq!(
                provider.stored(),
                1,
                "the original request can still reconcile"
            );
        }

        #[tokio::test]
        async fn test_success_and_failures_before_broadcast_record_nothing() {
            let provider = MockProvider::new(true);
            let sends = AtomicU32::new(0);
            assert_eq!(
                settle(&provider, REQUEST, &sends, || Ok(OTHER_TX))
                    .await
                    .unwrap(),
                OTHER_TX
            );
            let error = settle(&provider, OTHER_REQUEST, &sends, || {
                Err(Eip155ExactError::Transport(rpc_503()))
            })
            .await
            .unwrap_err();
            assert!(matches!(
                error,
                X402SchemeFacilitatorError::OnchainFailure(_)
            ));
            assert_eq!(provider.stored(), 0);
        }

        #[tokio::test]
        async fn test_without_a_store_every_request_settles_from_scratch() {
            let provider = MockProvider::new(false);
            let sends = AtomicU32::new(0);
            let error = settle(&provider, REQUEST, &sends, || Err(receipt_unavailable()))
                .await
                .unwrap_err();
            assert_pending_with(&error, TX_HASH);
            let _ = settle(&provider, REQUEST, &sends, || Err(receipt_unavailable())).await;
            assert_eq!(sends.load(Ordering::Relaxed), 2);
            assert_eq!(provider.receipt_lookups.load(Ordering::Relaxed), 0);
        }

        #[tokio::test]
        async fn test_arc_provider_passes_the_store_and_lookups_through() {
            let provider = Arc::new(MockProvider::new(true));
            assert!(provider.pending_settlements().is_some());
            provider.set_receipt(Some(receipt(TX_HASH, true)));
            assert!(provider.find_receipt(TX_HASH).await.is_some());
            assert_eq!(provider.receipt_lookups.load(Ordering::Relaxed), 1);
            provider.set_authorization_use(Some(settlement(|_| {})));
            assert!(
                provider
                    .find_authorization_use(authorization())
                    .await
                    .is_some()
            );
            assert_eq!(provider.authorization_lookups(), vec![authorization()]);
        }

        // ---- A settle request with no store entry whose authorization is already used ----

        const ASSET: Address = address!("0x036CbD53842c5426634e7929541eC2318f3dCF7e");
        const PAYER: Address = address!("0x1111111111111111111111111111111111111111");
        const SELLER: Address = address!("0x2222222222222222222222222222222222222222");
        const SIGNER: Address = address!("0x3333333333333333333333333333333333333333");
        const NONCE: B256 =
            b256!("0x6666666666666666666666666666666666666666666666666666666666666666");
        const VALUE: u64 = 10_000;
        /// The transaction that used the authorization, found on chain.
        const FOUND_TX: TxHash =
            b256!("0x7777777777777777777777777777777777777777777777777777777777777777");

        fn authorization() -> Eip3009Authorization {
            Eip3009Authorization {
                asset: ASSET,
                from: PAYER,
                to: SELLER,
                value: U256::from(VALUE),
                valid_after: UnixTimestamp::from_secs(1_000),
                valid_before: UnixTimestamp::from_secs(2_000),
                nonce: NONCE,
            }
        }

        /// A change to the JSON of a [`settlement`] receipt.
        type ReceiptChange = Box<dyn Fn(&mut serde_json::Value)>;

        /// The receipt of a settlement of [`authorization`] by [`SIGNER`], as `transferWithAuthorization`
        /// leaves it, after `change` has altered it.
        fn settlement(change: impl FnOnce(&mut serde_json::Value)) -> TransactionReceipt {
            let log = |index: u64, topics: serde_json::Value, data: String| {
                serde_json::json!({
                    "address": ASSET,
                    "topics": topics,
                    "data": data,
                    "blockHash": "0x1111111111111111111111111111111111111111111111111111111111111111",
                    "blockNumber": "0x10",
                    "transactionHash": FOUND_TX,
                    "transactionIndex": "0x0",
                    "logIndex": format!("0x{index:x}"),
                    "removed": false
                })
            };
            let mut receipt = serde_json::json!({
                "type": "0x2",
                "status": "0x1",
                "cumulativeGasUsed": "0x1",
                "logs": [
                    log(
                        0,
                        serde_json::json!([
                            IEIP3009::AuthorizationUsed::SIGNATURE_HASH,
                            PAYER.into_word(),
                            NONCE
                        ]),
                        "0x".to_string()
                    ),
                    log(
                        1,
                        serde_json::json!([
                            IEIP3009::Transfer::SIGNATURE_HASH,
                            PAYER.into_word(),
                            SELLER.into_word()
                        ]),
                        format!("0x{VALUE:064x}")
                    )
                ],
                "logsBloom": format!("0x{}", "0".repeat(512)),
                "transactionHash": FOUND_TX,
                "transactionIndex": "0x0",
                "blockHash": "0x1111111111111111111111111111111111111111111111111111111111111111",
                "blockNumber": "0x10",
                "gasUsed": "0x1",
                "effectiveGasPrice": "0x1",
                "from": SIGNER,
                "to": ASSET,
                "contractAddress": null
            });
            change(&mut receipt);
            serde_json::from_value(receipt).unwrap()
        }

        /// What settling the request from scratch gives once its authorization is used: the gas
        /// estimate of the new transaction reverts.
        fn authorization_used_error() -> Eip155ExactError {
            Eip155ExactError::Transport(TransportErrorKind::custom_str(
                "server returned an error response: error code 3: execution reverted: FiatTokenV2: authorization is used or canceled",
            ))
        }

        /// Settles a request through the store and the lookup, as the EIP-3009 settle paths do;
        /// `outcome` is what settling it from scratch gives.
        async fn settle_eip3009(
            provider: &MockProvider,
            authorization: Option<Eip3009Authorization>,
            sends: &AtomicU32,
            outcome: impl FnOnce() -> Result<TxHash, Eip155ExactError>,
        ) -> Result<TxHash, X402SchemeFacilitatorError> {
            settle_with_pending_store(provider, SIGNATURE, Some(REQUEST), "eip155:84532", || {
                sends.fetch_add(1, Ordering::Relaxed);
                settle_or_find_used_authorization(provider, authorization, async move { outcome() })
            })
            .await
        }

        fn assert_authorization_used_failure(error: &X402SchemeFacilitatorError) {
            assert!(
                matches!(
                    error,
                    X402SchemeFacilitatorError::OnchainFailure(message)
                        if message.contains("authorization is used or canceled")
                ),
                "expected the used-authorization failure, got {error:?}"
            );
        }

        #[tokio::test]
        async fn test_used_authorization_without_a_store_entry_answers_the_transaction_found() {
            let provider = MockProvider::new(true);
            provider.set_authorization_use(Some(settlement(|_| {})));
            let sends = AtomicU32::new(0);
            let tx_hash = settle_eip3009(&provider, Some(authorization()), &sends, || {
                Err(authorization_used_error())
            })
            .await
            .unwrap();
            assert_eq!(tx_hash, FOUND_TX);
            assert_eq!(provider.authorization_lookups(), vec![authorization()]);
            assert_eq!(sends.load(Ordering::Relaxed), 1);
            assert_eq!(provider.receipt_lookups.load(Ordering::Relaxed), 0);
            assert_eq!(provider.stored(), 0);
        }

        #[tokio::test]
        async fn test_used_authorization_after_a_failed_balance_check_answers_the_transaction_found()
         {
            // The first settlement can leave the payer without the balance the check wants.
            let provider = MockProvider::new(true);
            provider.set_authorization_use(Some(settlement(|_| {})));
            let sends = AtomicU32::new(0);
            let tx_hash = settle_eip3009(&provider, Some(authorization()), &sends, || {
                Err(PaymentVerificationError::InsufficientFunds.into())
            })
            .await
            .unwrap();
            assert_eq!(tx_hash, FOUND_TX);
        }

        #[tokio::test]
        async fn test_transaction_found_with_another_transfer_is_not_the_settlement() {
            let other = address!("0x9999999999999999999999999999999999999999");
            let changes: Vec<(&str, ReceiptChange)> = vec![
                (
                    "another value",
                    Box::new(|r| r["logs"][1]["data"] = format!("0x{:064x}", VALUE - 1).into()),
                ),
                (
                    "another recipient",
                    Box::new(move |r| {
                        r["logs"][1]["topics"][2] = serde_json::json!(other.into_word())
                    }),
                ),
                (
                    "another payer",
                    Box::new(move |r| {
                        r["logs"][1]["topics"][1] = serde_json::json!(other.into_word())
                    }),
                ),
                (
                    "a transfer of another token",
                    Box::new(move |r| r["logs"][1]["address"] = serde_json::json!(other)),
                ),
                (
                    "no transfer",
                    Box::new(|r| {
                        r["logs"].as_array_mut().unwrap().pop();
                    }),
                ),
            ];
            for (case, change) in changes {
                let provider = MockProvider::new(true);
                provider.set_authorization_use(Some(settlement(change)));
                let sends = AtomicU32::new(0);
                let error = settle_eip3009(&provider, Some(authorization()), &sends, || {
                    Err(authorization_used_error())
                })
                .await
                .expect_err(case);
                assert_authorization_used_failure(&error);
                assert_eq!(provider.authorization_lookups().len(), 1, "{case}");
            }
        }

        #[tokio::test]
        async fn test_transaction_found_that_is_not_ours_or_did_not_use_the_authorization_is_not_the_settlement()
         {
            let changes: Vec<(&str, ReceiptChange)> = vec![
                ("reverted", Box::new(|r| r["status"] = "0x0".into())),
                (
                    "sent by another address",
                    Box::new(|r| {
                        r["from"] = serde_json::json!(address!(
                            "0x9999999999999999999999999999999999999999"
                        ))
                    }),
                ),
                (
                    "another nonce used",
                    Box::new(|r| r["logs"][0]["topics"][2] = serde_json::json!(OTHER_REQUEST)),
                ),
                (
                    "no AuthorizationUsed",
                    Box::new(|r| {
                        r["logs"].as_array_mut().unwrap().remove(0);
                    }),
                ),
            ];
            for (case, change) in changes {
                let provider = MockProvider::new(true);
                provider.set_authorization_use(Some(settlement(change)));
                let sends = AtomicU32::new(0);
                let error = settle_eip3009(&provider, Some(authorization()), &sends, || {
                    Err(authorization_used_error())
                })
                .await
                .expect_err(case);
                assert_authorization_used_failure(&error);
            }
        }

        #[tokio::test]
        async fn test_transaction_found_without_its_receipt_is_pending_and_checked_again() {
            let provider = MockProvider::new(true);
            provider.set_authorization_use_without_receipt(FOUND_TX);
            let sends = AtomicU32::new(0);
            let error = settle_eip3009(&provider, Some(authorization()), &sends, || {
                Err(authorization_used_error())
            })
            .await
            .unwrap_err();
            assert_pending_with(&error, FOUND_TX);
            assert_eq!(provider.stored(), 0, "not recorded: it was not checked");
            // The next retry looks it up again, and answers once the receipt checks out.
            provider.set_authorization_use(Some(settlement(|_| {})));
            let tx_hash = settle_eip3009(&provider, Some(authorization()), &sends, || {
                Err(authorization_used_error())
            })
            .await
            .unwrap();
            assert_eq!(tx_hash, FOUND_TX);
            assert_eq!(provider.authorization_lookups().len(), 2);
        }

        #[tokio::test]
        async fn test_used_authorization_not_found_fails_as_before() {
            let provider = MockProvider::new(true);
            let sends = AtomicU32::new(0);
            let error = settle_eip3009(&provider, Some(authorization()), &sends, || {
                Err(authorization_used_error())
            })
            .await
            .unwrap_err();
            assert_authorization_used_failure(&error);
            assert_eq!(provider.authorization_lookups().len(), 1);
            assert_eq!(provider.stored(), 0);
        }

        #[tokio::test]
        async fn test_request_that_does_not_match_its_requirements_is_not_looked_up() {
            let provider = MockProvider::new(true);
            provider.set_authorization_use(Some(settlement(|_| {})));
            let sends = AtomicU32::new(0);
            let error = settle_eip3009(&provider, None, &sends, || {
                Err(PaymentVerificationError::RecipientMismatch.into())
            })
            .await
            .unwrap_err();
            assert!(matches!(
                error,
                X402SchemeFacilitatorError::PaymentVerification(
                    PaymentVerificationError::RecipientMismatch
                )
            ));
            assert!(provider.authorization_lookups().is_empty());
        }

        #[tokio::test]
        async fn test_success_and_pending_settlements_are_not_looked_up() {
            let provider = MockProvider::new(true);
            provider.set_authorization_use(Some(settlement(|_| {})));
            let sends = AtomicU32::new(0);
            let tx_hash = settle_eip3009(&provider, Some(authorization()), &sends, || Ok(OTHER_TX))
                .await
                .unwrap();
            assert_eq!(tx_hash, OTHER_TX);
            let error = settle_eip3009(&provider, Some(authorization()), &sends, || {
                Err(receipt_unavailable())
            })
            .await
            .unwrap_err();
            assert_pending_with(&error, TX_HASH);
            assert_eq!(provider.stored(), 1, "recorded for the retry, as before");
            assert!(provider.authorization_lookups().is_empty());
        }

        #[tokio::test]
        async fn test_retry_with_a_store_entry_reconciles_without_a_lookup() {
            let provider = MockProvider::new(true);
            provider.set_authorization_use(Some(settlement(|_| {})));
            let sends = AtomicU32::new(0);
            let _ = settle_eip3009(&provider, Some(authorization()), &sends, || {
                Err(receipt_unavailable())
            })
            .await;
            provider.set_receipt(Some(receipt(TX_HASH, true)));
            let tx_hash = settle_eip3009(&provider, Some(authorization()), &sends, || {
                panic!("a reconciled retry must not settle again")
            })
            .await
            .unwrap();
            assert_eq!(tx_hash, TX_HASH);
            assert!(provider.authorization_lookups().is_empty());
        }

        #[tokio::test]
        async fn test_same_signature_in_another_matching_request_answers_the_transaction_found() {
            // The trade-off `reconcile_used_authorizations` documents: with the lookup, a request
            // that carries a settled signature with other requirements (same network, asset and
            // recipient, an amount the authorization covers) is answered with the settlement
            // found on chain instead of failing on the used authorization, and the original
            // request keeps its store entry for its own retry.
            let provider = MockProvider::new(true);
            provider.set_authorization_use(Some(settlement(|_| {})));
            let sends = AtomicU32::new(0);
            let error = settle_eip3009(&provider, Some(authorization()), &sends, || {
                Err(receipt_unavailable())
            })
            .await
            .unwrap_err();
            assert_pending_with(&error, TX_HASH);
            let tx_hash = settle_with_pending_store(
                &provider,
                SIGNATURE,
                Some(OTHER_REQUEST),
                "eip155:84532",
                || {
                    sends.fetch_add(1, Ordering::Relaxed);
                    settle_or_find_used_authorization(&provider, Some(authorization()), async {
                        Err(authorization_used_error())
                    })
                },
            )
            .await
            .unwrap();
            assert_eq!(tx_hash, FOUND_TX);
            assert_eq!(provider.stored(), 1, "the original request keeps its entry");
            provider.set_receipt(Some(receipt(TX_HASH, true)));
            let tx_hash = settle_eip3009(&provider, Some(authorization()), &sends, || {
                panic!("a reconciled retry must not settle again")
            })
            .await
            .unwrap();
            assert_eq!(tx_hash, TX_HASH);
            assert_eq!(sends.load(Ordering::Relaxed), 2);
            assert_eq!(provider.authorization_lookups().len(), 1);
        }

        #[tokio::test]
        async fn test_providers_look_nothing_up_by_default() {
            /// Implements only what the trait requires.
            struct PlainProvider(RootProvider, Eip155ChainReference);
            impl Eip155MetaTransactionProvider for PlainProvider {
                type Error = MetaTransactionSendError;
                type Inner = RootProvider;
                fn inner(&self) -> &Self::Inner {
                    &self.0
                }
                fn chain(&self) -> &Eip155ChainReference {
                    &self.1
                }
                async fn send_transaction(
                    &self,
                    _tx: MetaTransaction,
                ) -> Result<TransactionReceipt, Self::Error> {
                    unreachable!()
                }
            }
            let provider = PlainProvider(
                RootProvider::new_http("http://127.0.0.1:9".parse().unwrap()),
                Eip155ChainReference::new(84532),
            );
            assert!(
                provider
                    .find_authorization_use(authorization())
                    .await
                    .is_none()
            );
        }

        fn v1_request(
            pay_to: Address,
            value: u64,
            network: &str,
        ) -> (types::PaymentPayload, types::PaymentRequirements) {
            let payload = serde_json::from_value(serde_json::json!({
                "x402Version": 1,
                "scheme": "exact",
                "network": "base-sepolia",
                "payload": {
                    "signature": "0x1234",
                    "authorization": {
                        "from": PAYER,
                        "to": SELLER,
                        "value": VALUE.to_string(),
                        "validAfter": "1000",
                        "validBefore": "2000",
                        "nonce": NONCE
                    }
                }
            }))
            .unwrap();
            let requirements = serde_json::from_value(serde_json::json!({
                "scheme": "exact",
                "network": network,
                "maxAmountRequired": value.to_string(),
                "resource": "https://example.com/data",
                "description": "",
                "mimeType": "application/json",
                "payTo": pay_to,
                "maxTimeoutSeconds": 60,
                "asset": ASSET,
                "extra": {"name": "USDC", "version": "2"}
            }))
            .unwrap();
            (payload, requirements)
        }

        #[test]
        fn test_v1_request_is_looked_up_only_when_it_matches_its_requirements() {
            let chain = Eip155ChainReference::new(84532);
            let (payload, requirements) = v1_request(SELLER, VALUE, "base-sepolia");
            assert_eq!(
                authorization_to_reconcile(&chain, &payload, &requirements),
                Some(authorization())
            );
            let (payload, requirements) = v1_request(SELLER, VALUE - 1, "base-sepolia");
            assert_eq!(
                authorization_to_reconcile(&chain, &payload, &requirements),
                Some(authorization()),
                "paying more than required is accepted, as in verification"
            );
            for (pay_to, value, network) in [
                (
                    address!("0x9999999999999999999999999999999999999999"),
                    VALUE,
                    "base-sepolia",
                ),
                (SELLER, VALUE + 1, "base-sepolia"),
                (SELLER, VALUE, "base"),
            ] {
                let (payload, requirements) = v1_request(pay_to, value, network);
                assert_eq!(
                    authorization_to_reconcile(&chain, &payload, &requirements),
                    None,
                    "{pay_to} {value} {network}"
                );
            }
        }
    }
}
