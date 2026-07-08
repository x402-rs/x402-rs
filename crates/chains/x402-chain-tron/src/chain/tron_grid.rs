//! HTTP client for the TronGrid full-node API, used with `visible: true` so all
//! addresses on the wire are Base58Check.

use alloy_primitives::Bytes;
use alloy_sol_types::SolCall;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::fmt::Formatter;
use std::ops::Deref;
use std::sync::Arc;
use std::time::Duration;
use url::Url;

use crate::chain::TronAddress;
use crate::chain::provider::TronChainProviderError;
use crate::chain::types::{TronTxId, prefixless_hex};

/// A TronGrid HTTP client bound to a single node's base URL.
pub struct TronGridHttp {
    /// TronGrid base URL.
    rpc_url: Url,
    /// HTTP client.
    client: reqwest::Client,
}

impl TronGridHttp {
    /// Creates a client with a fresh `reqwest::Client`.
    pub fn new(rpc_url: Url) -> Self {
        Self {
            rpc_url,
            client: reqwest::Client::new(),
        }
    }

    /// Creates a client reusing an existing `reqwest::Client` (e.g. for connection pooling).
    pub fn with_client(rpc_url: Url, client: reqwest::Client) -> Self {
        Self { rpc_url, client }
    }
}

impl fmt::Debug for TronGridHttp {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("TronGridHttp")
            .field("rpc_url", &self.rpc_url)
            .finish()
    }
}

/// Errors from calling the TronGrid HTTP API.
#[derive(Debug, thiserror::Error)]
pub enum TronGridLikeError {
    /// The HTTP request itself failed (network, URL parsing, etc).
    #[error("TronGrid transport: {0}")]
    Transport(String),
    /// The response body didn't have the expected shape.
    #[error("failed to parse TronGrid response: {0}")]
    ParsingError(String),
    /// TronGrid returned a well-formed response reporting failure.
    #[error("TronGrid returned an error: {0}")]
    ReportedError(String),
}

impl From<url::ParseError> for TronGridLikeError {
    fn from(value: url::ParseError) -> Self {
        Self::Transport(value.to_string())
    }
}

impl From<reqwest::Error> for TronGridLikeError {
    fn from(value: reqwest::Error) -> Self {
        Self::Transport(value.to_string())
    }
}

impl From<TronGridParsingError> for TronGridLikeError {
    fn from(value: TronGridParsingError) -> Self {
        Self::ParsingError(value.to_string())
    }
}

impl From<TronGridReportedError> for TronGridLikeError {
    fn from(value: TronGridReportedError) -> Self {
        Self::ReportedError(value.0)
    }
}

/// Internal parsing errors, converted into [`TronGridLikeError::ParsingError`].
#[derive(Debug, thiserror::Error)]
enum TronGridParsingError {
    #[error("missing field: {0}")]
    MissingField(String),
    #[error("can not abi decode: {0}")]
    AbiDecode(#[from] alloy_sol_types::Error),
}

/// A `result: false` reported by TronGrid, carrying its error message.
#[derive(Debug, thiserror::Error)]
#[error("TronGrid reported error: {0}")]
pub struct TronGridReportedError(String);

/// TronGrid wallet API operations needed to read state and submit transactions.
pub trait TronGridLike {
    /// Calls a read-only contract method via `triggerconstantcontract` and ABI-decodes
    /// the return value.
    fn wallet_trigger_constant_contract<TCalldata>(
        &self,
        contract_address: TronAddress,
        calldata: TCalldata,
        from: Option<TronAddress>,
    ) -> impl Future<Output = Result<TCalldata::Return, TronGridLikeError>>
    where
        TCalldata: SolCall + Send;

    /// Build an unsigned transaction via `triggersmartcontract`.
    ///
    /// Uses `visible: true` so addresses are Base58Check throughout.
    fn wallet_trigger_smart_contract<TCalldata>(
        &self,
        contract: TronAddress,
        calldata: TCalldata,
        owner: TronAddress,
    ) -> impl Future<Output = Result<TronTransaction, TronGridLikeError>>
    where
        TCalldata: SolCall;

    /// Broadcast a signed transaction.
    fn wallet_broadcast_transaction(
        &self,
        tx: TronTransaction,
    ) -> impl Future<Output = Result<TronTxId, TronGridLikeError>>;

    /// Fetches confirmation status via `gettransactioninfobyid`. Returns an empty
    /// response while the transaction is still pending.
    fn wallet_get_transaction_info_by_id(
        &self,
        tx_id: &TronTxId,
    ) -> impl Future<Output = Result<TransactionInfoResponse, TronGridLikeError>> + Send;
}

impl TronGridLike for TronGridHttp {
    async fn wallet_trigger_constant_contract<TCalldata>(
        &self,
        contract_address: TronAddress,
        calldata: TCalldata,
        from: Option<TronAddress>,
    ) -> Result<TCalldata::Return, TronGridLikeError>
    where
        TCalldata: SolCall + Send,
    {
        let url = self.rpc_url.join("wallet/triggerconstantcontract")?;
        let calldata = Bytes::from(calldata.abi_encode());
        let body = CallConstantRequest {
            owner_address: from.unwrap_or_default(),
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
        let decoded = resp.into_abi_decoded::<TCalldata>()?;
        Ok(decoded)
    }

    async fn wallet_trigger_smart_contract<TCalldata>(
        &self,
        contract: TronAddress,
        calldata: TCalldata,
        owner: TronAddress,
    ) -> Result<TronTransaction, TronGridLikeError>
    where
        TCalldata: SolCall,
    {
        let url = self.rpc_url.join("wallet/triggersmartcontract")?;
        let body = TriggerSmartContractRequest {
            owner_address: owner,
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

        let transaction = resp.try_into()?;
        Ok(transaction)
    }

    async fn wallet_broadcast_transaction(
        &self,
        tx: TronTransaction,
    ) -> Result<TronTxId, TronGridLikeError> {
        let url = self.rpc_url.join("wallet/broadcasttransaction")?;
        let resp: BroadcastResponse = self.client.post(url).json(&tx).send().await?.json().await?;
        let tx_id = resp.try_into()?;
        Ok(tx_id)
    }

    async fn wallet_get_transaction_info_by_id(
        &self,
        tx_id: &TronTxId,
    ) -> Result<TransactionInfoResponse, TronGridLikeError> {
        let url = self.rpc_url.join("wallet/gettransactioninfobyid")?;
        let body = GetTransactionInfoRequest { value: tx_id };
        let resp: TransactionInfoResponse = self
            .client
            .post(url)
            .json(&body)
            .send()
            .await?
            .json()
            .await?;
        Ok(resp)
    }
}

/// Wraps a [`TronGridLike`] client with the timeout/interval used to poll for
/// transaction confirmation.
pub struct TronGridPolling<A> {
    /// TronGrid client.
    pub tron_grid: A,
    /// How long to wait for a transaction to be confirmed before giving up.
    pub tx_timeout: Duration,
    /// How often to poll `gettransactioninfobyid`.
    pub tx_poll_interval: Duration,
}

impl<A> fmt::Debug for TronGridPolling<A>
where
    A: fmt::Debug,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.tron_grid.fmt(f)
    }
}

impl<A> Deref for TronGridPolling<A> {
    type Target = A;

    fn deref(&self) -> &Self::Target {
        &self.tron_grid
    }
}

/// Blocks (async) until a submitted transaction is confirmed on-chain.
pub trait WaitForTxLike {
    /// Polls until the transaction succeeds, fails, or times out.
    fn wait_for_tx(
        &self,
        tx_id: &TronTxId,
    ) -> impl Future<Output = Result<(), TronChainProviderError>> + Send;
}

impl<A> WaitForTxLike for Arc<A>
where
    A: WaitForTxLike,
{
    fn wait_for_tx(
        &self,
        tx_id: &TronTxId,
    ) -> impl Future<Output = Result<(), TronChainProviderError>> {
        self.as_ref().wait_for_tx(tx_id)
    }
}

impl<A> WaitForTxLike for TronGridPolling<A>
where
    A: TronGridLike + Sync,
{
    async fn wait_for_tx(&self, tx_id: &TronTxId) -> Result<(), TronChainProviderError> {
        let timeout = self.tx_timeout;
        let interval = self.tx_poll_interval;
        let start = std::time::Instant::now();
        loop {
            if start.elapsed() > timeout {
                return Err(TronChainProviderError::TxTimeout);
            }
            let transaction_info_response = self
                .tron_grid
                .wallet_get_transaction_info_by_id(tx_id)
                .await?;
            match transaction_info_response
                .receipt
                .as_ref()
                .and_then(|r| r.result.as_deref())
            {
                None => tokio::time::sleep(interval).await,
                Some("SUCCESS") => return Ok(()),
                Some(status) => return Err(TronChainProviderError::TxFailed(status.to_string())),
            }
        }
    }
}

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
    pub fn into_result(self) -> Result<(), TronGridReportedError> {
        if self.result {
            Ok(())
        } else {
            let message = self.message.unwrap_or_else(|| "unknown error".into());
            Err(TronGridReportedError(message))
        }
    }
}

/// Request body for `triggersmartcontract`.
#[derive(Debug, Serialize)]
pub struct TriggerSmartContractRequest {
    pub owner_address: TronAddress,
    pub contract_address: TronAddress,
    #[serde(with = "prefixless_hex")]
    pub data: Vec<u8>,
    pub fee_limit: u64,
    pub call_value: u64,
    pub visible: bool,
}

/// Request body for `gettransactioninfobyid`.
#[derive(Debug, Serialize)]
pub struct GetTransactionInfoRequest<'a> {
    pub value: &'a TronTxId,
}

/// An unsigned transaction returned by `triggersmartcontract`.
///
/// `signature` starts empty; `sign_and_broadcast` fills it before posting to
/// `broadcasttransaction`.  All other fields are captured in `rest` and
/// round-tripped verbatim so nothing is lost.
#[derive(Debug, Deserialize, Serialize)]
pub struct TronTransaction {
    #[serde(rename = "txID")]
    pub tx_id: TronTxId,
    #[serde(default, skip_serializing_if = "HexBytesVec::is_empty")]
    pub signature: HexBytesVec,
    #[serde(flatten)]
    pub rest: serde_json::Map<String, serde_json::Value>,
}

/// Response from `triggersmartcontract`.
#[derive(Debug, Deserialize)]
pub struct TriggerSmartContractResponse {
    pub result: TriggerStatus,
    pub transaction: Option<TronTransaction>,
}

impl TryFrom<TriggerSmartContractResponse> for TronTransaction {
    type Error = TronGridLikeError;

    fn try_from(value: TriggerSmartContractResponse) -> Result<Self, Self::Error> {
        value.result.into_result()?;
        let transaction = value
            .transaction
            .ok_or_else(|| TronGridParsingError::MissingField("transaction".to_string()))?;
        Ok(transaction)
    }
}

/// Response from `broadcasttransaction`.
/// Note: `result` here is a flat `bool`, not a nested object.
#[derive(Debug, Deserialize)]
pub struct BroadcastResponse {
    pub result: bool,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub txid: Option<TronTxId>,
}

impl TryFrom<BroadcastResponse> for TronTxId {
    type Error = TronGridLikeError;

    fn try_from(value: BroadcastResponse) -> Result<Self, Self::Error> {
        if !value.result {
            let msg = value.message.unwrap_or_else(|| "broadcast failed".into());
            return Err(TronGridLikeError::ReportedError(msg));
        }
        let tx_id = value
            .txid
            .ok_or_else(|| TronGridParsingError::MissingField("txid".to_string()))?;
        Ok(tx_id)
    }
}

/// Response from `gettransactioninfobyid`.
/// All fields are optional — an empty object `{}` means the tx is still pending.
#[derive(Debug, Deserialize)]
pub struct TransactionInfoResponse {
    #[serde(default)]
    pub receipt: Option<TxReceipt>,
}

#[derive(Debug, Deserialize)]
pub struct TxReceipt {
    pub result: Option<String>,
}

// ── Serde helpers ─────────────────────────────────────────────────────────────

/// Request body for `triggerconstantcontract`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallConstantRequest {
    pub owner_address: TronAddress,
    pub contract_address: TronAddress,
    #[serde(with = "prefixless_hex")]
    pub data: Bytes,
    pub call_value: u64,
    pub visible: bool,
}

/// Response from `triggerconstantcontract`.
#[derive(Debug, Deserialize)]
pub struct CallConstantResponse {
    pub result: TriggerStatus,
    #[serde(default)]
    pub constant_result: HexBytesVec,
}

impl CallConstantResponse {
    /// ABI-decodes the first `constant_result` entry as `TCalldata`'s return type.
    pub fn into_abi_decoded<TCalldata: SolCall>(
        self,
    ) -> Result<TCalldata::Return, TronGridLikeError> {
        self.result.into_result()?;
        let constant_result = self
            .constant_result
            .0
            .first()
            .ok_or_else(|| TronGridParsingError::MissingField("constant_result".to_string()))?;

        let decoded = TCalldata::abi_decode_returns(constant_result)
            .map_err(TronGridParsingError::AbiDecode)?;
        Ok(decoded)
    }
}

/// A list of byte strings, (de)serialized as prefixless hex strings.
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
