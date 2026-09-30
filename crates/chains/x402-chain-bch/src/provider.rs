//! UTXO and settlement provider interfaces for BCH.
//!
//! The included Fulcrum adapter speaks the Electrum Cash JSON-RPC protocol.
//! It is intentionally transport-generic so applications can supply a TLS or
//! WebSocket transport when their Fulcrum deployment requires one. The bundled
//! TCP transport is useful for local/private deployments.

use async_trait::async_trait;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use x402_types::chain::{ChainId, ChainProviderOps};

use crate::address::{CashAddr, p2pkh_script};
use crate::chain::BchChainReference;
use crate::transaction::{
    BchNft, BchToken, BchTokenCapability, OutPoint, SourceOutput, TxId, is_p2pkh_script,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BchUtxo {
    pub outpoint: OutPoint,
    pub source_output: SourceOutput,
    pub height: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BchTransactionStatus {
    NotFound,
    Mempool,
    Confirmed { height: u64 },
    Conflicted,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BchOutpointStatus {
    Unspent,
    Spent,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum BchProviderError {
    #[error("provider transport error: {0}")]
    Transport(String),
    #[error("provider returned an RPC error ({code}): {message}")]
    Remote { code: i64, message: String },
    #[error("provider returned malformed data: {0}")]
    InvalidResponse(String),
    #[error("provider could not find the requested source output")]
    NotFound,
    #[error(transparent)]
    Address(#[from] crate::address::CashAddrError),
    #[error(transparent)]
    Transaction(#[from] crate::transaction::TransactionError),
}

/// Chain operations required by the BCH exact client and facilitator.
#[async_trait]
pub trait BchChainProvider: ChainProviderOps + Send + Sync {
    async fn source_output(&self, outpoint: &OutPoint) -> Result<SourceOutput, BchProviderError>;

    async fn outpoint_status(
        &self,
        outpoint: &OutPoint,
        source_output: &SourceOutput,
    ) -> Result<BchOutpointStatus, BchProviderError>;

    async fn list_utxos(&self, address: &CashAddr) -> Result<Vec<BchUtxo>, BchProviderError>;

    async fn broadcast(&self, transaction: &[u8]) -> Result<TxId, BchProviderError>;

    async fn transaction_status(
        &self,
        txid: &TxId,
    ) -> Result<BchTransactionStatus, BchProviderError>;

    async fn tip_height(&self) -> Result<u64, BchProviderError>;

    /// Returns whether the provider has BCH double-spend-proof evidence for a
    /// transaction. The provider must document whether it validates the proof
    /// cryptographically or merely reports node/indexer state.
    async fn has_double_spend_proof(&self, txid: &TxId) -> Result<bool, BchProviderError>;
}

/// Minimal JSON-RPC transport contract for Fulcrum-compatible servers.
#[async_trait]
pub trait FulcrumTransport: Clone + Send + Sync + 'static {
    async fn request(&self, method: &str, params: Value) -> Result<Value, BchProviderError>;
}

/// Sequentially retries Fulcrum requests across caller-provided transports.
///
/// Endpoint construction, TLS certificate validation, and endpoint ordering
/// remain application responsibilities. This helper only provides availability
/// failover; it does not validate chain consistency or SPV proofs.
#[derive(Clone)]
pub struct FailoverFulcrumTransport<T> {
    transports: Vec<T>,
}

impl<T> FailoverFulcrumTransport<T> {
    pub fn new(transports: Vec<T>) -> Result<Self, BchProviderError> {
        if transports.is_empty() {
            return Err(BchProviderError::InvalidResponse(
                "at least one Fulcrum transport is required".to_string(),
            ));
        }
        Ok(Self { transports })
    }
}

#[async_trait]
impl<T: FulcrumTransport> FulcrumTransport for FailoverFulcrumTransport<T> {
    async fn request(&self, method: &str, params: Value) -> Result<Value, BchProviderError> {
        let mut errors = Vec::new();
        for transport in &self.transports {
            match transport.request(method, params.clone()).await {
                Ok(value) => return Ok(value),
                Err(error) => errors.push(error.to_string()),
            }
        }
        Err(BchProviderError::Transport(format!(
            "all Fulcrum transports failed: {}",
            errors.join("; ")
        )))
    }
}

/// A newline-delimited Electrum JSON-RPC connection.
#[derive(Clone)]
pub struct FulcrumTcpTransport {
    stream: Arc<Mutex<TcpStream>>,
    next_id: Arc<AtomicU64>,
}

impl FulcrumTcpTransport {
    pub async fn connect(address: &str) -> Result<Self, BchProviderError> {
        let stream = TcpStream::connect(address)
            .await
            .map_err(|error| BchProviderError::Transport(error.to_string()))?;
        Ok(Self {
            stream: Arc::new(Mutex::new(stream)),
            next_id: Arc::new(AtomicU64::new(1)),
        })
    }
}

#[async_trait]
impl FulcrumTransport for FulcrumTcpTransport {
    async fn request(&self, method: &str, params: Value) -> Result<Value, BchProviderError> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        });
        let encoded = serde_json::to_vec(&request)
            .map_err(|error| BchProviderError::InvalidResponse(error.to_string()))?;
        let mut stream = self.stream.lock().await;
        stream
            .write_all(&encoded)
            .await
            .map_err(|error| BchProviderError::Transport(error.to_string()))?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|error| BchProviderError::Transport(error.to_string()))?;
        stream
            .flush()
            .await
            .map_err(|error| BchProviderError::Transport(error.to_string()))?;

        loop {
            let mut response = Vec::new();
            loop {
                let byte = stream
                    .read_u8()
                    .await
                    .map_err(|error| BchProviderError::Transport(error.to_string()))?;
                if byte == b'\n' {
                    break;
                }
                if response.len() >= 8 * 1024 * 1024 {
                    return Err(BchProviderError::InvalidResponse(
                        "Fulcrum response exceeds maximum size".to_string(),
                    ));
                }
                response.push(byte);
            }
            let response: Value = serde_json::from_slice(&response)
                .map_err(|error| BchProviderError::InvalidResponse(error.to_string()))?;
            // Fulcrum sends subscription notifications on the same connection.
            // Ignore those until the response for this request ID arrives.
            if response.get("id").and_then(Value::as_u64) != Some(id) {
                continue;
            }
            if let Some(error) = response.get("error") {
                let code = error.get("code").and_then(Value::as_i64).unwrap_or(-1);
                let message = error
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown Fulcrum error")
                    .to_string();
                return Err(BchProviderError::Remote { code, message });
            }
            return response.get("result").cloned().ok_or_else(|| {
                BchProviderError::InvalidResponse("missing JSON-RPC result".to_string())
            });
        }
    }
}

/// Fulcrum Electrum Cash provider.
#[derive(Clone)]
pub struct FulcrumProvider<T> {
    transport: T,
    network: BchChainReference,
}

impl<T> FulcrumProvider<T> {
    pub fn new(transport: T, network: BchChainReference) -> Self {
        Self { transport, network }
    }

    pub fn network(&self) -> BchChainReference {
        self.network
    }
}

impl<T> ChainProviderOps for FulcrumProvider<T> {
    fn signer_addresses(&self) -> Vec<String> {
        Vec::new()
    }

    fn chain_id(&self) -> ChainId {
        self.network.into()
    }
}

#[async_trait]
impl<T: FulcrumTransport> BchChainProvider for FulcrumProvider<T> {
    async fn source_output(&self, outpoint: &OutPoint) -> Result<SourceOutput, BchProviderError> {
        let transaction = self
            .transport
            .request(
                "blockchain.transaction.get",
                json!([outpoint.txid.to_string(), true]),
            )
            .await?;
        let output = transaction
            .get("vout")
            .and_then(Value::as_array)
            .and_then(|outputs| {
                outputs.iter().find(|output| {
                    output.get("n").and_then(Value::as_u64) == Some(u64::from(outpoint.vout))
                })
            })
            .ok_or(BchProviderError::NotFound)?;
        let token = parse_token_data(output.get("tokenData").or_else(|| output.get("token_data")))?;
        let script_hex = output
            .get("scriptPubKey")
            .and_then(|script| script.get("hex"))
            .and_then(Value::as_str)
            .ok_or_else(|| {
                BchProviderError::InvalidResponse("missing output script".to_string())
            })?;
        let script_pubkey = hex::decode(script_hex).map_err(|_| {
            BchProviderError::InvalidResponse("invalid output script hex".to_string())
        })?;
        Ok(SourceOutput {
            value: parse_bch_amount(output.get("value").ok_or_else(|| {
                BchProviderError::InvalidResponse("missing output value".to_string())
            })?)?,
            script_pubkey,
            token,
        })
    }

    async fn outpoint_status(
        &self,
        outpoint: &OutPoint,
        source_output: &SourceOutput,
    ) -> Result<BchOutpointStatus, BchProviderError> {
        if !is_p2pkh_script(&source_output.script_pubkey) {
            return Ok(BchOutpointStatus::Unknown);
        }
        let mut script_hash = Sha256::digest(&source_output.script_pubkey);
        script_hash.reverse();
        let result = self
            .transport
            .request(
                "blockchain.scripthash.listunspent",
                json!([hex::encode(script_hash), "include_tokens"]),
            )
            .await?;
        let entries = result.as_array().ok_or_else(|| {
            BchProviderError::InvalidResponse("listunspent is not an array".to_string())
        })?;
        let unspent = entries.iter().any(|entry| {
            let txid_matches = entry
                .get("tx_hash")
                .or_else(|| entry.get("txid"))
                .and_then(Value::as_str)
                .is_some_and(|value| value.eq_ignore_ascii_case(&outpoint.txid.to_string()));
            let vout_matches = entry
                .get("tx_pos")
                .or_else(|| entry.get("vout"))
                .and_then(Value::as_u64)
                == Some(u64::from(outpoint.vout));
            txid_matches && vout_matches
        });
        Ok(if unspent {
            BchOutpointStatus::Unspent
        } else {
            BchOutpointStatus::Spent
        })
    }

    async fn list_utxos(&self, address: &CashAddr) -> Result<Vec<BchUtxo>, BchProviderError> {
        if address.network != self.network {
            return Err(BchProviderError::InvalidResponse(
                "address network mismatch".to_string(),
            ));
        }
        let script = p2pkh_script(&address.hash160);
        let mut script_hash = Sha256::digest(&script);
        script_hash.reverse();
        let result = self
            .transport
            .request(
                "blockchain.scripthash.listunspent",
                json!([hex::encode(script_hash), "include_tokens"]),
            )
            .await?;
        let entries = result.as_array().ok_or_else(|| {
            BchProviderError::InvalidResponse("listunspent is not an array".to_string())
        })?;
        entries
            .iter()
            .map(|entry| {
                let txid = entry
                    .get("tx_hash")
                    .or_else(|| entry.get("txid"))
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        BchProviderError::InvalidResponse("missing UTXO txid".to_string())
                    })?;
                let vout = entry
                    .get("tx_pos")
                    .or_else(|| entry.get("vout"))
                    .and_then(Value::as_u64)
                    .ok_or_else(|| {
                        BchProviderError::InvalidResponse("missing UTXO index".to_string())
                    })?;
                let height = entry.get("height").and_then(Value::as_i64);
                let token =
                    parse_token_data(entry.get("tokenData").or_else(|| entry.get("token_data")))?;
                Ok(BchUtxo {
                    outpoint: OutPoint {
                        txid: TxId::from_hex(txid)?,
                        vout: u32::try_from(vout).map_err(|_| {
                            BchProviderError::InvalidResponse("UTXO index exceeds u32".to_string())
                        })?,
                    },
                    source_output: SourceOutput {
                        value: parse_satoshi_amount(entry.get("value").ok_or_else(|| {
                            BchProviderError::InvalidResponse("missing UTXO value".to_string())
                        })?)?,
                        script_pubkey: script.clone(),
                        token,
                    },
                    height: height
                        .filter(|height| *height > 0)
                        .map(|height| height as u64),
                })
            })
            .collect()
    }

    async fn broadcast(&self, transaction: &[u8]) -> Result<TxId, BchProviderError> {
        let raw = hex::encode(transaction);
        let txid = self
            .transport
            .request("blockchain.transaction.broadcast", json!([raw]))
            .await?
            .as_str()
            .ok_or_else(|| {
                BchProviderError::InvalidResponse("broadcast did not return a txid".to_string())
            })?
            .to_string();
        TxId::from_hex(&txid).map_err(Into::into)
    }

    async fn transaction_status(
        &self,
        txid: &TxId,
    ) -> Result<BchTransactionStatus, BchProviderError> {
        match self
            .transport
            .request(
                "blockchain.transaction.get_height",
                json!([txid.to_string()]),
            )
            .await
        {
            Ok(value) => {
                let height = value.as_i64().ok_or_else(|| {
                    BchProviderError::InvalidResponse("invalid transaction height".to_string())
                })?;
                if height > 0 {
                    Ok(BchTransactionStatus::Confirmed {
                        height: height as u64,
                    })
                } else if height == 0 || height == -1 {
                    Ok(BchTransactionStatus::Mempool)
                } else {
                    Ok(BchTransactionStatus::NotFound)
                }
            }
            Err(BchProviderError::Remote { code: -5, .. }) => Ok(BchTransactionStatus::NotFound),
            Err(error) => Err(error),
        }
    }

    async fn tip_height(&self) -> Result<u64, BchProviderError> {
        self.transport
            .request("blockchain.headers.subscribe", json!([]))
            .await?
            .get("height")
            .and_then(Value::as_u64)
            .ok_or_else(|| BchProviderError::InvalidResponse("invalid chain tip".to_string()))
    }

    async fn has_double_spend_proof(&self, txid: &TxId) -> Result<bool, BchProviderError> {
        let result = self
            .transport
            .request(
                "blockchain.transaction.dsproof.get",
                json!([txid.to_string()]),
            )
            .await?;
        Ok(!result.is_null() && result != Value::String(String::new()))
    }
}

fn parse_bch_amount(value: &Value) -> Result<u64, BchProviderError> {
    let text = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => {
            return Err(BchProviderError::InvalidResponse(
                "invalid BCH amount".to_string(),
            ));
        }
    };
    let exponent_separator = text.find('e').or_else(|| text.find('E'));
    let (mantissa, exponent) = if let Some(index) = exponent_separator {
        let exponent = text[index + 1..].parse::<i32>().map_err(|_| {
            BchProviderError::InvalidResponse("invalid BCH amount exponent".to_string())
        })?;
        (&text[..index], exponent)
    } else {
        (text.as_str(), 0)
    };
    if exponent.unsigned_abs() > 100 {
        return Err(BchProviderError::InvalidResponse(
            "invalid BCH amount exponent".to_string(),
        ));
    }

    let (whole, fractional) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    if whole.is_empty()
        || (mantissa.contains('.') && fractional.is_empty())
        || !whole.bytes().all(|byte| byte.is_ascii_digit())
        || !fractional.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(BchProviderError::InvalidResponse(
            "invalid BCH amount".to_string(),
        ));
    }
    let mut digits = whole.to_string();
    digits.push_str(fractional);
    let unscaled = digits
        .parse::<u128>()
        .map_err(|_| BchProviderError::InvalidResponse("BCH amount overflow".to_string()))?;
    if unscaled == 0 {
        return Ok(0);
    }
    let decimal_places = fractional.len() as i32 - exponent;
    let amount = if decimal_places <= 8 {
        let scale = u32::try_from(8 - decimal_places)
            .map_err(|_| BchProviderError::InvalidResponse("BCH amount overflow".to_string()))?;
        unscaled
            .checked_mul(10u128.pow(scale))
            .ok_or_else(|| BchProviderError::InvalidResponse("BCH amount overflow".to_string()))?
    } else {
        let scale = u32::try_from(decimal_places - 8)
            .map_err(|_| BchProviderError::InvalidResponse("BCH amount overflow".to_string()))?;
        let divisor = 10u128
            .checked_pow(scale)
            .ok_or_else(|| BchProviderError::InvalidResponse("BCH amount overflow".to_string()))?;
        if unscaled % divisor != 0 {
            return Err(BchProviderError::InvalidResponse(
                "BCH amount has more than 8 decimals".to_string(),
            ));
        }
        unscaled / divisor
    };
    u64::try_from(amount)
        .map_err(|_| BchProviderError::InvalidResponse("BCH amount overflow".to_string()))
}

fn parse_satoshi_amount(value: &Value) -> Result<u64, BchProviderError> {
    let text = match value {
        Value::String(value) => value.clone(),
        Value::Number(value) => value.to_string(),
        _ => {
            return Err(BchProviderError::InvalidResponse(
                "invalid BCH satoshi amount".to_string(),
            ));
        }
    };
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(BchProviderError::InvalidResponse(
            "invalid BCH satoshi amount".to_string(),
        ));
    }
    text.parse::<u64>()
        .map_err(|_| BchProviderError::InvalidResponse("BCH satoshi amount overflow".to_string()))
}

fn parse_token_data(value: Option<&Value>) -> Result<Option<BchToken>, BchProviderError> {
    let Some(value) = value.filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    let category = value
        .get("category")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            BchProviderError::InvalidResponse("missing CashToken category".to_string())
        })?;
    let category_bytes = hex::decode(category)
        .map_err(|_| BchProviderError::InvalidResponse("invalid CashToken category".to_string()))?;
    if category_bytes.len() != 32 {
        return Err(BchProviderError::InvalidResponse(
            "CashToken category must be 32 bytes".to_string(),
        ));
    }
    let mut category_array = [0u8; 32];
    category_array.copy_from_slice(&category_bytes);
    let amount = value
        .get("amount")
        .map(parse_token_amount)
        .transpose()?
        .unwrap_or(0);
    let nft = value
        .get("nft")
        .filter(|value| !value.is_null())
        .map(|nft| {
            if !nft.is_object() {
                return Err(BchProviderError::InvalidResponse(
                    "invalid CashToken NFT".to_string(),
                ));
            }
            let capability = match nft
                .get("capability")
                .and_then(Value::as_str)
                .unwrap_or("none")
            {
                "none" => BchTokenCapability::None,
                "mutable" => BchTokenCapability::Mutable,
                "minting" => BchTokenCapability::Minting,
                _ => {
                    return Err(BchProviderError::InvalidResponse(
                        "invalid CashToken NFT capability".to_string(),
                    ));
                }
            };
            let commitment = nft
                .get("commitmentHex")
                .or_else(|| nft.get("commitment"))
                .and_then(Value::as_str)
                .map(hex::decode)
                .transpose()
                .map_err(|_| {
                    BchProviderError::InvalidResponse("invalid CashToken commitment".to_string())
                })?
                .unwrap_or_default();
            if commitment.len() > 40 {
                return Err(BchProviderError::InvalidResponse(
                    "CashToken commitment is too large".to_string(),
                ));
            }
            Ok(BchNft {
                capability,
                commitment,
            })
        })
        .transpose()?;
    if amount == 0 && nft.is_none() {
        return Err(BchProviderError::InvalidResponse(
            "CashToken output has no token data".to_string(),
        ));
    }
    Ok(Some(BchToken {
        category: category_array,
        amount,
        nft,
    }))
}

fn parse_token_amount(value: &Value) -> Result<u64, BchProviderError> {
    let text = value
        .as_str()
        .map(str::to_owned)
        .or_else(|| value.as_u64().map(|value| value.to_string()))
        .ok_or_else(|| BchProviderError::InvalidResponse("invalid CashToken amount".to_string()))?;
    if !text.chars().all(|character| character.is_ascii_digit())
        || (text.len() > 1 && text.starts_with('0'))
    {
        return Err(BchProviderError::InvalidResponse(
            "invalid CashToken amount".to_string(),
        ));
    }
    let amount = text
        .parse::<u64>()
        .map_err(|_| BchProviderError::InvalidResponse("invalid CashToken amount".to_string()))?;
    if amount > i64::MAX as u64 {
        return Err(BchProviderError::InvalidResponse(
            "CashToken amount exceeds BCH limits".to_string(),
        ));
    }
    Ok(amount)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct TestTransport {
        fail: bool,
        calls: Arc<Mutex<Vec<String>>>,
    }

    #[async_trait]
    impl FulcrumTransport for TestTransport {
        async fn request(&self, method: &str, _params: Value) -> Result<Value, BchProviderError> {
            self.calls.lock().unwrap().push(method.to_string());
            if self.fail {
                Err(BchProviderError::Transport("offline".to_string()))
            } else {
                Ok(json!({"height": 123}))
            }
        }
    }

    #[test]
    fn parses_exact_bch_decimal_amounts_without_floating_point() {
        assert_eq!(parse_bch_amount(&json!("1.00000001")).unwrap(), 100_000_001);
        assert_eq!(parse_bch_amount(&json!("0.00000001")).unwrap(), 1);
        assert_eq!(parse_bch_amount(&json!(1e-8)).unwrap(), 1);
        assert!(parse_bch_amount(&json!("1.000000001")).is_err());
        assert_eq!(parse_satoshi_amount(&json!(1)).unwrap(), 1);
        assert_eq!(
            parse_satoshi_amount(&json!("100000000")).unwrap(),
            100_000_000
        );
        assert!(parse_satoshi_amount(&json!("0.00000001")).is_err());
    }

    #[test]
    fn fails_over_to_the_next_transport() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let first = TestTransport {
            fail: true,
            calls: calls.clone(),
        };
        let second = TestTransport {
            fail: false,
            calls: calls.clone(),
        };
        let transport = FailoverFulcrumTransport::new(vec![first, second]).unwrap();
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let result = runtime.block_on(transport.request("blockchain.headers.subscribe", json!([])));

        assert_eq!(result.unwrap(), json!({"height": 123}));
        assert_eq!(
            *calls.lock().unwrap(),
            vec![
                "blockchain.headers.subscribe".to_string(),
                "blockchain.headers.subscribe".to_string()
            ]
        );
    }

    #[test]
    fn rejects_an_empty_failover_set() {
        assert!(FailoverFulcrumTransport::<TestTransport>::new(vec![]).is_err());
    }

    #[test]
    #[ignore = "requires BCH_FULCRUM_TCP_ENDPOINT for a live Chipnet provider"]
    fn live_chipnet_fulcrum_tcp_smoke() {
        let endpoint = std::env::var("BCH_FULCRUM_TCP_ENDPOINT")
            .expect("BCH_FULCRUM_TCP_ENDPOINT must be set for the live smoke test");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let (tip, transaction_status, source_status, source_value) = runtime.block_on(async {
            let transport = FulcrumTcpTransport::connect(&endpoint).await.unwrap();
            let provider = FulcrumProvider::new(transport, BchChainReference::CHIPNET);
            let payment_txid =
                TxId::from_hex("449fc5076c65559e77df28a071e8bab6c6ae2954a2e10583dc6d0899f7df5e45")
                    .unwrap();
            let source_outpoint = OutPoint {
                txid: TxId::from_hex(
                    "cd895e0def451b2f4b331d5f0d77d82d51146fe6a25c4c9af4d54f6f4c472488",
                )
                .unwrap(),
                vout: 1,
            };
            let source = provider.source_output(&source_outpoint).await.unwrap();
            let transaction_status = provider.transaction_status(&payment_txid).await.unwrap();
            let source_status = provider
                .outpoint_status(&source_outpoint, &source)
                .await
                .unwrap();
            (
                provider.tip_height().await.unwrap(),
                transaction_status,
                source_status,
                source.value,
            )
        });
        assert!(tip > 0, "Chipnet tip must be positive");
        assert!(matches!(
            transaction_status,
            BchTransactionStatus::Confirmed { height } if height > 0
        ));
        assert_eq!(source_status, BchOutpointStatus::Spent);
        assert_eq!(source_value, 9631);
    }

    #[test]
    #[ignore = "requires BCH_FULCRUM_TCP_ENDPOINT for a live Chipnet CashToken smoke test"]
    fn live_chipnet_fulcrum_cash_token_smoke() {
        let endpoint = std::env::var("BCH_FULCRUM_TCP_ENDPOINT")
            .expect("BCH_FULCRUM_TCP_ENDPOINT must be set for the live smoke test");
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async {
            let transport = FulcrumTcpTransport::connect(&endpoint).await.unwrap();
            let provider = FulcrumProvider::new(transport, BchChainReference::CHIPNET);
            let payment_txid =
                TxId::from_hex("cacb2d3115ccd6a7814cfe8d6fada54fc84db394069dd65d55e71b4b20630862")
                    .unwrap();
            let source_outpoint = OutPoint {
                txid: TxId::from_hex(
                    "12a6e3ab4174b8b10b88f6e62d7de1d4e4153ac90fe3296f1de4f7c5591a467b",
                )
                .unwrap(),
                vout: 0,
            };
            let source = provider.source_output(&source_outpoint).await.unwrap();
            let token = source.token.as_ref().unwrap();
            assert_eq!(token.amount, 1_000_000);
            assert_eq!(
                provider
                    .outpoint_status(&source_outpoint, &source)
                    .await
                    .unwrap(),
                BchOutpointStatus::Spent
            );
            assert!(matches!(
                provider.transaction_status(&payment_txid).await.unwrap(),
                BchTransactionStatus::Mempool | BchTransactionStatus::Confirmed { .. }
            ));
            let payer = CashAddr::decode(
                "bchtest:qr4rj7f4hu64a8vwzvq64ev3qgm64gxjjqujw020e5",
                BchChainReference::CHIPNET,
            )
            .unwrap();
            let change = provider
                .list_utxos(&payer)
                .await
                .unwrap()
                .into_iter()
                .find(|utxo| {
                    utxo.outpoint
                        == OutPoint {
                            txid: payment_txid,
                            vout: 1,
                        }
                })
                .unwrap();
            assert_eq!(change.source_output.value, 8_696);
            assert_eq!(change.source_output.token.unwrap().amount, 900_000);
        });
    }
}
