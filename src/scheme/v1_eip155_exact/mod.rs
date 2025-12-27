use alloy_contract::SolCallBuilder;
use alloy_primitives::{Address, B256, Bytes, Signature, TxHash, U256, address, hex};
use alloy_provider::bindings::IMulticall3;
use alloy_provider::{
    MULTICALL3_ADDRESS, MulticallError, MulticallItem, PendingTransactionError, Provider,
};
use alloy_sol_types::{Eip712Domain, SolCall, SolStruct, SolType, eip712_domain, sol};
use alloy_transport::TransportError;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::Instrument;
use tracing::instrument;
use tracing_core::Level;

pub mod client;
pub mod types;

use crate::chain::eip155::{
    Eip155ChainProvider, Eip155ChainReference, Eip155MetaTransactionProvider, MetaTransaction,
    MetaTransactionSendError,
};
use crate::chain::{ChainId, ChainProvider, ChainProviderOps};
use crate::proto;
use crate::proto::{PaymentVerificationError, v1};
use crate::scheme::{
    X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError, X402SchemeId,
};
use crate::timestamp::UnixTimestamp;

pub use types::*;

/// Signature verifier for EIP-6492, EIP-1271, EOA, universally deployed on the supported EVM chains
/// If absent on a target chain, verification will fail; you should deploy the validator there.
pub const VALIDATOR_ADDRESS: Address = address!("0xdAcD51A54883eb67D95FAEb2BBfdC4a9a6BD2a3B");

pub struct V1Eip155Exact;

impl X402SchemeId for V1Eip155Exact {
    fn x402_version(&self) -> u8 {
        1
    }
    fn namespace(&self) -> &str {
        "eip155"
    }
    fn scheme(&self) -> &str {
        ExactScheme.as_ref()
    }
}

impl X402SchemeFacilitatorBuilder for V1Eip155Exact {
    fn build(
        &self,
        provider: ChainProvider,
        _config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let provider = if let ChainProvider::Eip155(provider) = provider {
            provider
        } else {
            return Err("V1Eip155Exact::build: provider must be an Eip155ChainProvider".into());
        };
        Ok(Box::new(V1Eip155ExactFacilitator { provider }))
    }
}

pub struct V1Eip155ExactFacilitator {
    provider: Arc<Eip155ChainProvider>,
}

#[async_trait::async_trait]
impl X402SchemeFacilitator for V1Eip155ExactFacilitator {
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError> {
        let request = types::VerifyRequest::from_proto(request.clone())?;
        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;
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

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, X402SchemeFacilitatorError> {
        let request = types::SettleRequest::from_proto(request.clone())?;
        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;
        let (contract, payment, eip712_domain) = assert_valid_payment(
            self.provider.inner(),
            self.provider.chain(),
            payload,
            requirements,
        )
        .await?;

        let tx_hash =
            settle_payment(self.provider.as_ref(), &contract, &payment, &eip712_domain).await?;
        Ok(v1::SettleResponse::Success {
            payer: payment.from.to_string(),
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
#[instrument(skip_all, err)]
async fn assert_valid_payment<P: Provider>(
    provider: P,
    chain: &Eip155ChainReference,
    payload: &types::PaymentPayload,
    requirements: &types::PaymentRequirements,
) -> Result<(IEIP3009::IEIP3009Instance<P>, ExactEvmPayment, Eip712Domain), Eip155ExactError> {
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

/// Validates that the current time is within the `validAfter` and `validBefore` bounds.
///
/// Adds a 6-second grace buffer when checking expiration to account for latency.
#[instrument(skip_all, err)]
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
#[instrument(skip_all, err, fields(
    network = %chain.as_chain_id(),
    asset = %asset_address
))]
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
        token_contract
            .name()
            .call()
            .into_future()
            .instrument(tracing::info_span!(
                "fetch_eip712_name",
                otel.kind = "client",
            ))
            .await?
    };
    let version = extra.as_ref().map(|extra| extra.version.clone());
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
            .await?
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
#[instrument(skip_all, err, fields(
    sender = %sender,
    max_required = %max_amount_required,
    token_contract = %ieip3009_token_contract.address()
))]
pub async fn assert_enough_balance<P: Provider>(
    ieip3009_token_contract: &IEIP3009::IEIP3009Instance<P>,
    sender: &Address,
    max_amount_required: U256,
) -> Result<(), Eip155ExactError> {
    let balance = ieip3009_token_contract
        .balanceOf(*sender)
        .call()
        .into_future()
        .instrument(tracing::info_span!(
            "fetch_token_balance",
            token_contract = %ieip3009_token_contract.address(),
            sender = %sender,
            otel.kind = "client"
        ))
        .await?;

    if balance < max_amount_required {
        Err(PaymentVerificationError::InsufficientFunds.into())
    } else {
        Ok(())
    }
}

/// Verifies that the declared `value` in the payload is sufficient for the required amount.
///
/// This is a static check (not on-chain) that compares two numbers.
#[instrument(skip_all, err, fields(
    sent = %sent,
    max_amount_required = %max_amount_required
))]
pub fn assert_enough_value(
    sent: &U256,
    max_amount_required: &U256,
) -> Result<(), PaymentVerificationError> {
    if sent < max_amount_required {
        Err(PaymentVerificationError::InvalidPaymentAmount)
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
    pub fn extract(
        payment: &ExactEvmPayment,
        domain: &Eip712Domain,
    ) -> Result<Self, StructuredSignatureFormatError> {
        let transfer_with_authorization = TransferWithAuthorization {
            from: payment.from,
            to: payment.to,
            value: payment.value,
            validAfter: U256::from(payment.valid_after.as_secs()),
            validBefore: U256::from(payment.valid_before.as_secs()),
            nonce: payment.nonce,
        };
        let eip712_hash = transfer_with_authorization.eip712_signing_hash(domain);
        let structured_signature: StructuredSignature = StructuredSignature::try_from_bytes(
            payment.signature.clone(),
            payment.from,
            &eip712_hash,
        )?;
        let signed_message = Self {
            address: payment.from,
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
    /// Normalized EOA signature.
    #[allow(clippy::upper_case_acronyms)]
    EOA(Signature),
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
                        StructuredSignature::EOA(s)
                    } else {
                        StructuredSignature::EIP1271(bytes)
                    }
                }
            }
        };
        Ok(signature)
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
        let v = 27 + (signature.v() as u8);
        let r = B256::from(signature.r());
        let s = B256::from(signature.s());
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
async fn is_contract_deployed<P: Provider>(
    provider: P,
    address: &Address,
) -> Result<bool, TransportError> {
    let bytes = provider
        .get_code_at(*address)
        .into_future()
        .instrument(tracing::info_span!("get_code_at",
            address = %address,
            otel.kind = "client",
        ))
        .await?;
    Ok(!bytes.is_empty())
}

pub async fn verify_payment<P: Provider>(
    provider: P,
    contract: &IEIP3009::IEIP3009Instance<P>,
    payment: &ExactEvmPayment,
    eip712_domain: &Eip712Domain,
) -> Result<Address, Eip155ExactError> {
    let signed_message = SignedMessage::extract(payment, eip712_domain)?;

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
            let (is_valid_signature_result, transfer_result) = provider
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
                .await?;
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
                .await?;
        }
        StructuredSignature::EOA(signature) => {
            // It is EOA signature, which we can pass to the transfer simulation of (r,s,v)-based transferWithAuthorization function
            let transfer_call = TransferWithAuthorization1Call::new(contract, payment, signature);
            let transfer_call = transfer_call.0;
            transfer_call
                .tx
                .call()
                .into_future()
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
        }
    }

    Ok(payer)
}

pub async fn settle_payment<P, E>(
    provider: P,
    contract: &IEIP3009::IEIP3009Instance<&P::Inner>,
    payment: &ExactEvmPayment,
    eip712_domain: &Eip712Domain,
) -> Result<TxHash, Eip155ExactError>
where
    P: Eip155MetaTransactionProvider<Error = E>,
    Eip155ExactError: From<E>,
{
    let signed_message = SignedMessage::extract(payment, eip712_domain)?;
    let payer = payment.from;
    let transaction_receipt_fut = match signed_message.signature {
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
                Eip155MetaTransactionProvider::send_transaction(
                    &provider,
                    MetaTransaction {
                        to: transfer_call.tx.target(),
                        calldata: transfer_call.tx.calldata().clone(),
                        confirmations: 1,
                    },
                )
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
                Eip155MetaTransactionProvider::send_transaction(
                    &provider,
                    MetaTransaction {
                        to: MULTICALL3_ADDRESS,
                        calldata: aggregate_call.abi_encode().into(),
                        confirmations: 1,
                    },
                )
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
                TransferWithAuthorization0Call::new(contract, payment, eip1271_signature);
            let transfer_call = transfer_call.0;
            // transferWithAuthorization with eip1271 signature
            Eip155MetaTransactionProvider::send_transaction(
                &provider,
                MetaTransaction {
                    to: transfer_call.tx.target(),
                    calldata: transfer_call.tx.calldata().clone(),
                    confirmations: 1,
                },
            )
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
        StructuredSignature::EOA(signature) => {
            let transfer_call = TransferWithAuthorization1Call::new(contract, payment, signature);
            let transfer_call = transfer_call.0;
            // transferWithAuthorization with EOA signature
            Eip155MetaTransactionProvider::send_transaction(
                &provider,
                MetaTransaction {
                    to: transfer_call.tx.target(),
                    calldata: transfer_call.tx.calldata().clone(),
                    confirmations: 1,
                },
            )
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
        Ok(receipt.transaction_hash)
    } else {
        tracing::event!(
            Level::WARN,
            status = "failed",
            tx = %receipt.transaction_hash,
            "transferWithAuthorization_0 failed"
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
    #[error("Transaction {0} reverted")]
    TransactionReverted(TxHash),
    #[error("Contract call failed: {0}")]
    ContractCall(String),
    #[error(transparent)]
    PaymentVerification(#[from] PaymentVerificationError),
}

impl From<Eip155ExactError> for X402SchemeFacilitatorError {
    fn from(value: Eip155ExactError) -> Self {
        match value {
            Eip155ExactError::Transport(_) => Self::OnchainFailure(value.to_string()),
            Eip155ExactError::PendingTransaction(_) => Self::OnchainFailure(value.to_string()),
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
