//! TRON chain provider for x402 payments.
//!
//! Communicates with the TRON blockchain via the TronGrid HTTP API using
//! `visible: true`, which means all addresses are passed and returned as
//! Base58Check strings (the canonical TRON format).

use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::SolCall;
use k256::ecdsa::{RecoveryId, SigningKey, VerifyingKey};
use reqwest::Client;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::time::Duration;
use url::Url;
use x402_types::chain::{ChainId, ChainProviderOps, FromConfig};

use crate::chain::TronAddress;
use crate::chain::config::{TronChainConfig, TronPrivateKey};
use crate::chain::contracts;
use crate::chain::types::TronChainReference;

// ── TronGrid response types ───────────────────────────────────────────────────

/// The nested `result` object inside `trigger*` responses.
/// Distinct from `broadcasttransaction` which has a flat `bool` at `result`.
#[derive(Debug, Deserialize)]
pub struct TriggerStatus {
    result: bool,
    #[serde(default)]
    message: Option<String>,
}

impl TriggerStatus {
    fn into_result(self) -> Result<(), String> {
        if self.result {
            Ok(())
        } else {
            Err(self.message.unwrap_or_else(|| "unknown error".into()))
        }
    }
}

/// Request body for `triggersmartcontract`.
#[derive(Debug, Serialize)]
struct TriggerSmartContractRequest {
    owner_address: TronAddress,
    contract_address: TronAddress,
    #[serde(with = "prefixless_hex")]
    data: Vec<u8>,
    fee_limit: u64,
    call_value: u64,
    visible: bool,
}

/// An unsigned transaction returned by `triggersmartcontract`.
///
/// `signature` starts empty; `sign_and_broadcast` fills it before posting to
/// `broadcasttransaction`.  All other fields are captured in `rest` and
/// round-tripped verbatim so nothing is lost.
#[derive(Debug, Deserialize, Serialize)]
struct TronTransaction {
    #[serde(rename = "txID")]
    tx_id: String,
    #[serde(default, skip_serializing_if = "HexBytesVec::is_empty")]
    signature: HexBytesVec,
    #[serde(flatten)]
    rest: serde_json::Map<String, Value>,
}

/// Response from `triggersmartcontract`.
#[derive(Debug, Deserialize)]
struct TriggerSmartContractResponse {
    result: TriggerStatus,
    transaction: Option<TronTransaction>,
}

/// Response from `broadcasttransaction`.
/// Note: `result` here is a flat `bool`, not a nested object.
#[derive(Debug, Deserialize)]
struct BroadcastResponse {
    result: bool,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    txid: Option<String>,
}

/// Response from `gettransactioninfobyid`.
/// All fields are optional — an empty object `{}` means the tx is still pending.
#[derive(Debug, Deserialize)]
struct TransactionInfoResponse {
    #[serde(default)]
    receipt: Option<TxReceipt>,
}

#[derive(Debug, Deserialize)]
struct TxReceipt {
    result: Option<String>,
}

// ── Errors ────────────────────────────────────────────────────────────────────

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

struct TronSigner {
    signing_key: SigningKey,
    address: TronAddress,
}

impl TronSigner {
    fn from_key(key: &TronPrivateKey) -> Result<Self, TronChainProviderError> {
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

    pub fn address(&self) -> &TronAddress {
        &self.address
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

// ── TronChainProvider ─────────────────────────────────────────────────────────

/// TRON chain provider.
///
/// Wraps TronGrid HTTP API (`visible: true`) and one or more k256 signing keys.
pub struct TronChainProvider {
    /// Chain reference for this provider.
    pub chain_reference: TronChainReference,
    /// TronGrid base URL.
    pub rpc_url: Url,
    /// HTTP client.
    pub client: Client,
    /// SUN.io Permit2 contract — the EIP-712 `verifyingContract` that clients sign against.
    pub sun_permit2: TronAddress,
    /// x402ExactPermit2Proxy — the `spender` in Permit2 messages and the settlement contract.
    pub x402_exact_permit2_proxy: TronAddress,
    /// All configured signers (at least one required).
    signers: Vec<TronSigner>,
}

impl fmt::Debug for TronChainProvider {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("TronChainProvider")
            .field("chain_reference", &self.chain_reference)
            .field("rpc_url", &self.rpc_url)
            .field(
                "signer_addresses",
                &self
                    .signers
                    .iter()
                    .map(|s| s.address.to_string())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl TronChainProvider {
    /// Returns the Base58Check address of the first (active) signer.
    pub fn facilitator_address(&self) -> TronAddress {
        self.signers[0].address // TODO Multiple addresses
    }

    /// Returns the EVM address of the first (active) signer.
    pub fn facilitator_evm(&self) -> Address {
        Address::from(self.signers[0].address) // TODO Multiple addresses and why do we need this at all?
    }

    // ── TronGrid HTTP helpers ─────────────────────────────────────────────────

    /// Build an unsigned transaction via `triggersmartcontract`.
    ///
    /// Uses `visible: true` so addresses are Base58Check throughout.
    async fn build_tx<TCalldata: SolCall>(
        &self,
        contract: TronAddress,
        calldata: TCalldata,
    ) -> Result<TronTransaction, TronChainProviderError> {
        let url = self
            .rpc_url
            .join("wallet/triggersmartcontract")
            .map_err(|e| TronChainProviderError::Api(e.to_string()))?;
        let body = TriggerSmartContractRequest {
            owner_address: self.facilitator_address(),
            contract_address: contract,
            data: calldata.abi_encode(),
            fee_limit: 100_000_000,
            call_value: 0,
            visible: true,
        };
        let resp: TriggerSmartContractResponse = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        resp.result
            .into_result()
            .map_err(TronChainProviderError::Api)?;

        resp.transaction.ok_or_else(|| {
            TronChainProviderError::Api("missing transaction in response".to_string())
        })
    }

    /// Sign a transaction's `txID` and return a 65-byte TRON signature hex.
    ///
    /// Format: r(32) + s(32) + (recovery_id + 27)(1).
    fn sign_tx(&self, txid_hex: &str) -> Result<[u8; 65], TronChainProviderError> {
        let txid_bytes = alloy_primitives::hex::decode(txid_hex)
            .map_err(|e| TronChainProviderError::InvalidKey(format!("bad txid: {e}")))?;
        let (sig, recid): (k256::ecdsa::Signature, RecoveryId) = self.signers[0]
            .signing_key
            .sign_prehash_recoverable(&txid_bytes)
            .map_err(|e| TronChainProviderError::InvalidKey(format!("sign failed: {e}")))?;
        let mut sig_bytes = [0u8; 65];
        sig_bytes[..64].copy_from_slice(&sig.to_bytes());
        sig_bytes[64] = recid.to_byte() + 27;
        Ok(sig_bytes)
    }

    /// Broadcast a signed transaction.
    async fn broadcast(&self, tx: TronTransaction) -> Result<String, TronChainProviderError> {
        let url = self
            .rpc_url
            .join("wallet/broadcasttransaction")
            .map_err(|e| TronChainProviderError::Api(e.to_string()))?;
        let resp: BroadcastResponse = self.client.post(url).json(&tx).send().await?.json().await?;
        if !resp.result {
            let msg = resp.message.unwrap_or_else(|| "broadcast failed".into());
            return Err(TronChainProviderError::Api(msg));
        }
        resp.txid.ok_or_else(|| {
            TronChainProviderError::Api("missing txid in broadcast response".to_string())
        })
    }

    /// Sign and broadcast, returning the txid.
    async fn sign_and_broadcast(
        &self,
        mut tx: TronTransaction,
    ) -> Result<String, TronChainProviderError> {
        let signature = self.sign_tx(&tx.tx_id)?;
        let signature = Bytes::from(signature);
        tx.signature = HexBytesVec(vec![signature]);
        self.broadcast(tx).await
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
            .collect::<Result<Vec<_>, _>>()?;

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

        let rpc_url = config.inner.rpc_url.inner().clone();

        Ok(Self {
            chain_reference,
            rpc_url,
            signers,
            client: Client::new(),
            x402_exact_permit2_proxy,
            sun_permit2,
        })
    }
}

impl ChainProviderOps for TronChainProvider {
    fn signer_addresses(&self) -> Vec<String> {
        self.signers
            .iter()
            .map(|s| s.address().to_string())
            .collect()
    }

    fn chain_id(&self) -> ChainId {
        self.chain_reference.chain_id()
    }
}

pub trait TronChainProviderLike {
    fn is_signer(&self, addr: &TronAddress) -> bool;
    fn chain(&self) -> &TronChainReference;
    fn trigger_constant_contract<TCalldata>(
        &self,
        contract: TronAddress,
        calldata: TCalldata,
        from: Option<TronAddress>,
    ) -> impl Future<Output = Result<TCalldata::Return, TronChainProviderError>> + Send
    where
        TCalldata: SolCall + Send;
    fn build_and_submit_tx<TCalldata>(
        &self,
        contract: TronAddress,
        calldata: TCalldata,
    ) -> impl Future<Output = Result<String, TronChainProviderError>> + Send
    where
        TCalldata: SolCall + Send;
    fn wait_for_tx(
        &self,
        txid: &str,
    ) -> impl Future<Output = Result<(), TronChainProviderError>> + Send;
}

impl TronChainProviderLike for TronChainProvider {
    fn is_signer(&self, addr: &TronAddress) -> bool {
        self.signers.iter().any(|s| s.address == *addr)
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
        let url = self
            .rpc_url
            .join("wallet/triggerconstantcontract")
            .map_err(|e| TronChainProviderError::Api(e.to_string()))?;
        let calldata = Bytes::from(calldata.abi_encode());
        let body = CallConstantRequest {
            owner_address: from.unwrap_or_else(|| TronAddress::default()),
            contract_address,
            data: calldata,
            call_value: 0,
            visible: true,
        };
        let resp: CallConstantResponse = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;

        resp.result
            .into_result()
            .map_err(TronChainProviderError::Api)?;
        let constant_result =
            resp.constant_result.0.first().ok_or_else(|| {
                TronChainProviderError::Api("missing constant_result".to_string())
            })?;

        let decoded = TCalldata::abi_decode_returns(&constant_result)
            .map_err(|e| TronChainProviderError::AbiDecode(e.to_string()))?;

        Ok(decoded)
    }

    async fn build_and_submit_tx<TCalldata>(
        &self,
        contract: TronAddress,
        calldata: TCalldata,
    ) -> Result<String, TronChainProviderError>
    where
        TCalldata: SolCall + Send,
    {
        let tx = self.build_tx(contract, calldata).await?;
        self.sign_and_broadcast(tx).await
    }

    async fn wait_for_tx(&self, txid: &str) -> Result<(), TronChainProviderError> {
        let url = self
            .rpc_url
            .join("wallet/gettransactioninfobyid")
            .map_err(|e| TronChainProviderError::Api(e.to_string()))?;
        let body = serde_json::json!({ "value": txid });
        let timeout = Duration::from_secs(60);
        let interval = Duration::from_secs(3); // FIXME CONFIGURABLE
        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > timeout {
                return Err(TronChainProviderError::TxTimeout);
            }
            let resp: TransactionInfoResponse = self
                .client
                .post(url.clone())
                .json(&body)
                .send()
                .await?
                .json()
                .await?;
            match resp.receipt.as_ref().and_then(|r| r.result.as_deref()) {
                None => tokio::time::sleep(interval).await,
                Some("SUCCESS") => return Ok(()),
                Some(status) => return Err(TronChainProviderError::TxFailed(status.to_string())),
            }
        }
    }
}

// ── ERC20 reads (used by both EIP-3009 and Permit2 facilitators) ──────────────

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

// ── Serde helpers ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallConstantRequest {
    pub owner_address: TronAddress,
    pub contract_address: TronAddress,
    #[serde(with = "prefixless_hex")]
    pub data: Bytes,
    pub call_value: u64,
    pub visible: bool,
}

#[derive(Debug, Deserialize)]
pub struct CallConstantResponse {
    pub result: TriggerStatus,
    #[serde(default)]
    pub constant_result: HexBytesVec,
}

#[derive(Debug, Default)]
pub struct HexBytesVec(pub Vec<Bytes>);

impl HexBytesVec {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl Serialize for HexBytesVec {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeSeq;
        let mut seq = serializer.serialize_seq(Some(self.0.len()))?;
        for value in &self.0 {
            seq.serialize_element(&prefixless_hex::PrefixlessHex(value))?;
        }
        seq.end()
    }
}

impl<'de> Deserialize<'de> for HexBytesVec {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct PrefixlessHexVecVisitor;

        impl<'de> serde::de::Visitor<'de> for PrefixlessHexVecVisitor {
            type Value = HexBytesVec;

            fn expecting(&self, formatter: &mut Formatter) -> fmt::Result {
                formatter.write_str("a list of prefixless hex strings")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut values = Vec::new();

                while let Some(value) = seq.next_element::<prefixless_hex::PrefixlessHexOwned>()? {
                    values.push(value.0);
                }

                Ok(HexBytesVec(values))
            }
        }

        deserializer.deserialize_seq(PrefixlessHexVecVisitor)
    }
}

pub mod prefixless_hex {
    use alloy_primitives::Bytes;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub struct PrefixlessHex<'a>(pub &'a [u8]);

    impl Serialize for PrefixlessHex<'_> {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            serialize(self.0, serializer)
        }
    }

    pub struct PrefixlessHexOwned(pub Bytes);

    impl<'de> Deserialize<'de> for PrefixlessHexOwned {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserialize(deserializer).map(Self)
        }
    }

    pub fn serialize<S>(value: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let value = alloy_primitives::hex::encode(value).replace("0x", "");
        serializer.serialize_str(&value)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Bytes, D::Error>
    where
        D: Deserializer<'de>,
    {
        let as_string = String::deserialize(deserializer)?;
        let vec = alloy_primitives::hex::decode(&as_string).map_err(serde::de::Error::custom)?;
        let bytes = Bytes::from(vec);
        Ok(bytes)
    }
}
