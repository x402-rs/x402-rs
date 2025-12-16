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

use alloy_network::EthereumWallet;
use alloy_primitives::{hex};
use alloy_primitives::{Bytes, FixedBytes};
use alloy_provider::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller,
};
use alloy_provider::{
    Identity,  RootProvider
};
use alloy_sol_types::SolType;
use alloy_sol_types::sol;
use alloy_sol_types::{Eip712Domain, SolCall, SolStruct};
use std::fmt::{Display, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize};

use crate::chain::{FacilitatorLocalError};
use crate::facilitator::Facilitator;
use crate::network::{Network};
use crate::timestamp::UnixTimestamp;
use crate::types::{
    EvmSignature, HexEncodedNonce,
    TokenAmount, TransferWithAuthorization,
};

use crate::p1::chain::eip155::{Eip155ChainReference};
use crate::p1::chain::{ChainId, ChainIdError};
pub use alloy_primitives::Address;
use crate::p1::chain::eip155::pending_nonce_manager::PendingNonceManager;

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