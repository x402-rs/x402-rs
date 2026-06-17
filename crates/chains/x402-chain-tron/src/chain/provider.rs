//! TRON chain provider for x402 payments.
//!
//! Communicates with the TRON blockchain via the TronGrid HTTP API using
//! `visible: true`, which means all addresses are passed and returned as
//! Base58Check strings (the canonical TRON format).

use alloy_primitives::{Address, B256, Bytes, U256};
use alloy_sol_types::{SolCall, sol};
use k256::ecdsa::{RecoveryId, SigningKey, VerifyingKey};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::fmt::{Debug, Display, Formatter};
use std::time::Duration;
use url::Url;
use x402_types::chain::{ChainId, ChainProviderOps, FromConfig};
use x402_types::timestamp::UnixTimestamp;

use crate::chain::TronAddress;
use crate::chain::config::{TronChainConfig, TronPrivateKey};
use crate::chain::types::TronChainReference;

sol! {
    function balanceOf(address account) external view returns (uint256);
    function allowance(address owner, address spender) external view returns (uint256);
    function authorizationState(address authorizer, bytes32 nonce) external view returns (bool);
    function transferWithAuthorization(
        address from,
        address to,
        uint256 value,
        uint256 validAfter,
        uint256 validBefore,
        bytes32 nonce,
        bytes calldata signature
    ) external;
}

sol! {
    struct TronTokenPermissions {
        address token;
        uint256 amount;
    }

    struct TronPermitTransferFrom {
        TronTokenPermissions permitted;
        uint256 nonce;
        uint256 deadline;
    }

    struct TronWitness {
        address to;
        uint256 validAfter;
    }

    function settle(
        TronPermitTransferFrom permit,
        address owner,
        TronWitness witness,
        bytes signature
    ) external;
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
    fn into_result(self) -> Result<(), String> {
        if self.result {
            Ok(())
        } else {
            Err(self.message.unwrap_or_else(|| "unknown error".into()))
        }
    }
}

/// Response from `triggerconstantcontract`.
#[derive(Debug, Deserialize)]
struct ConstantContractResponse {
    result: TriggerStatus,
    #[serde(default)]
    constant_result: Vec<String>,
}

/// Response from `triggersmartcontract`.
/// The `transaction` field is kept as raw JSON because we add `signature` to it
/// before broadcasting.
#[derive(Debug, Deserialize)]
struct SmartContractResponse {
    result: TriggerStatus,
    transaction: Option<Value>,
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
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "TronSigner {{ address: {:?} }}", self.address)
    }
}

impl Display for TronSigner {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
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

    /// Returns true if the given EVM address belongs to any configured signer.
    pub fn is_signer(&self, addr: &Address) -> bool {
        let tron_addr = TronAddress::from(addr);
        self.signers.iter().any(|s| s.address == tron_addr)
    }

    // ── TronGrid HTTP helpers ─────────────────────────────────────────────────

    /// Call a contract read-only method via `triggerconstantcontract`.
    ///
    /// Uses `visible: true` so addresses are Base58Check throughout.
    pub async fn call_constant(
        &self,
        contract: &TronAddress,
        calldata: &[u8],
    ) -> Result<Vec<u8>, TronChainProviderError> {
        let url = self
            .rpc_url
            .join("wallet/triggerconstantcontract")
            .map_err(|e| TronChainProviderError::Api(e.to_string()))?;
        let body = serde_json::json!({
            "owner_address": self.facilitator_address().to_string(),
            "contract_address": contract.to_string(),
            "data": alloy_primitives::hex::encode(calldata),
            "call_value": 0,
            "visible": true
        });
        let resp: ConstantContractResponse = self
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

        let hex_result = resp
            .constant_result
            .first()
            .ok_or_else(|| TronChainProviderError::Api("missing constant_result".to_string()))?;

        alloy_primitives::hex::decode(hex_result)
            .map_err(|e| TronChainProviderError::AbiDecode(e.to_string()))
    }

    /// Build an unsigned transaction via `triggersmartcontract`.
    ///
    /// Uses `visible: true` so addresses are Base58Check throughout.
    async fn build_tx(
        &self,
        contract: &TronAddress,
        calldata: &[u8],
    ) -> Result<Value, TronChainProviderError> {
        let url = self
            .rpc_url
            .join("wallet/triggersmartcontract")
            .map_err(|e| TronChainProviderError::Api(e.to_string()))?;
        let body = serde_json::json!({
            "owner_address": self.facilitator_address().to_string(),
            "contract_address": contract.to_string(),
            "data": alloy_primitives::hex::encode(calldata),
            "fee_limit": 100_000_000u64,
            "call_value": 0,
            "visible": true
        });
        let resp: SmartContractResponse = self
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
    fn sign_tx(&self, txid_hex: &str) -> Result<String, TronChainProviderError> {
        let txid_bytes = alloy_primitives::hex::decode(txid_hex)
            .map_err(|e| TronChainProviderError::InvalidKey(format!("bad txid: {e}")))?;
        let (sig, recid): (k256::ecdsa::Signature, RecoveryId) = self.signers[0]
            .signing_key
            .sign_prehash_recoverable(&txid_bytes)
            .map_err(|e| TronChainProviderError::InvalidKey(format!("sign failed: {e}")))?;
        let mut sig_bytes = [0u8; 65];
        sig_bytes[..64].copy_from_slice(&sig.to_bytes());
        sig_bytes[64] = recid.to_byte() + 27;
        Ok(alloy_primitives::hex::encode(sig_bytes))
    }

    /// Broadcast a signed transaction.
    async fn broadcast(&self, tx: Value) -> Result<String, TronChainProviderError> {
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
    async fn sign_and_broadcast(&self, mut tx: Value) -> Result<String, TronChainProviderError> {
        let txid = tx
            .get("txID")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TronChainProviderError::Api("missing txID in transaction".to_string()))?
            .to_string();
        tx["signature"] = serde_json::json!([self.sign_tx(&txid)?]);
        self.broadcast(tx).await
    }

    /// Poll `gettransactioninfobyid` until confirmed, failed, or timed out.
    pub async fn wait_for_tx(&self, txid: &str) -> Result<(), TronChainProviderError> {
        let url = self
            .rpc_url
            .join("wallet/gettransactioninfobyid")
            .map_err(|e| TronChainProviderError::Api(e.to_string()))?;
        let body = serde_json::json!({ "value": txid });
        let timeout = Duration::from_secs(60);
        let interval = Duration::from_secs(3);
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
                None => tokio::time::sleep(interval).await, // pending: no receipt yet
                Some("SUCCESS") => return Ok(()),
                Some(status) => return Err(TronChainProviderError::TxFailed(status.to_string())),
            }
        }
    }

    // ── High-level on-chain operations ────────────────────────────────────────

    pub async fn read_balance_of(
        &self,
        token: &TronAddress,
        owner_evm: Address,
    ) -> Result<U256, TronChainProviderError> {
        let result = self
            .call_constant(token, &balanceOfCall { account: owner_evm }.abi_encode())
            .await?;
        if result.len() < 32 {
            return Err(TronChainProviderError::AbiDecode(
                "balanceOf result too short".into(),
            ));
        }
        Ok(U256::from_be_slice(&result[result.len() - 32..]))
    }

    pub async fn read_allowance(
        &self,
        token: &TronAddress,
        owner_evm: Address,
        spender_evm: Address,
    ) -> Result<U256, TronChainProviderError> {
        let result = self
            .call_constant(
                token,
                &allowanceCall {
                    owner: owner_evm,
                    spender: spender_evm,
                }
                .abi_encode(),
            )
            .await?;
        if result.len() < 32 {
            return Err(TronChainProviderError::AbiDecode(
                "allowance result too short".into(),
            ));
        }
        Ok(U256::from_be_slice(&result[result.len() - 32..]))
    }

    pub async fn read_authorization_state(
        &self,
        token: &TronAddress,
        authorizer_evm: Address,
        nonce: B256,
    ) -> Result<bool, TronChainProviderError> {
        let result = self
            .call_constant(
                token,
                &authorizationStateCall {
                    authorizer: authorizer_evm,
                    nonce,
                }
                .abi_encode(),
            )
            .await?;
        if result.len() < 32 {
            return Err(TronChainProviderError::AbiDecode(
                "authorizationState result too short".into(),
            ));
        }
        Ok(result[result.len() - 1] != 0)
    }

    pub async fn simulate_transfer_with_authorization(
        &self,
        token: &TronAddress,
        from: Address,
        to: Address,
        value: U256,
        valid_after: UnixTimestamp,
        valid_before: UnixTimestamp,
        nonce: B256,
        signature: Bytes,
    ) -> Result<bool, TronChainProviderError> {
        let calldata = transferWithAuthorizationCall {
            from,
            to,
            value,
            validAfter: U256::from(valid_after.as_secs()),
            validBefore: U256::from(valid_before.as_secs()),
            nonce,
            signature,
        }
        .abi_encode();
        match self.call_constant(token, &calldata).await {
            Ok(_) => Ok(true),
            Err(TronChainProviderError::Api(_)) => Ok(false),
            Err(e) => Err(e),
        }
    }

    pub async fn build_and_submit_eip3009_tx(
        &self,
        token: &TronAddress,
        from: Address,
        to: Address,
        value: U256,
        valid_after: UnixTimestamp,
        valid_before: UnixTimestamp,
        nonce: B256,
        signature: Bytes,
    ) -> Result<String, TronChainProviderError> {
        let calldata = transferWithAuthorizationCall {
            from,
            to,
            value,
            validAfter: U256::from(valid_after.as_secs()),
            validBefore: U256::from(valid_before.as_secs()),
            nonce,
            signature,
        }
        .abi_encode();
        let tx = self.build_tx(token, &calldata).await?;
        self.sign_and_broadcast(tx).await
    }

    pub async fn build_and_submit_permit2_settle_tx(
        &self,
        x402_exact_permit2_proxy: &TronAddress,
        token: Address,
        amount: U256,
        nonce: U256,
        deadline: UnixTimestamp,
        owner: Address,
        witness_to: Address,
        witness_valid_after: UnixTimestamp,
        signature: Bytes,
    ) -> Result<String, TronChainProviderError> {
        let calldata = settleCall {
            permit: TronPermitTransferFrom {
                permitted: TronTokenPermissions { token, amount },
                nonce,
                deadline: U256::from(deadline.as_secs()),
            },
            owner,
            witness: TronWitness {
                to: witness_to,
                validAfter: U256::from(witness_valid_after.as_secs()),
            },
            signature,
        }
        .abi_encode();
        let tx = self.build_tx(x402_exact_permit2_proxy, &calldata).await?;
        self.sign_and_broadcast(tx).await
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
        let x402_exact_permit2_proxy = contracts.and_then(|c| c.x402_exact_permit2_proxy).or_else(|| chain_reference.x402_exact_permit2_proxy()).ok_or(
            TronChainProviderError::Api(format!("can not get x402ExactPermit2Proxy contract address for tron:{chain_reference}"))
        )?;
        let sun_permit2 = contracts.and_then(|c| c.sun_permit2).or_else(|| chain_reference.sun_permit2()).ok_or(TronChainProviderError::Api(format!("can not get Permit2 contract address for tron:{chain_reference}")))?;

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
    /// Returns true if the given EVM address belongs to any configured signer.
    fn is_signer(&self, addr: &TronAddress) -> bool;
    fn chain(&self) -> &TronChainReference;
    fn call_constant_a<TCalldata>(
        &self,
        contract: TronAddress,
        calldata: TCalldata,
        from: Option<TronAddress>,
    ) -> impl Future<Output = Result<Vec<u8>, TronChainProviderError>> + Send
    where
        TCalldata: SolCall + Send;
}

impl TronChainProviderLike for TronChainProvider {
    fn is_signer(&self, addr: &TronAddress) -> bool {
        self.signers.iter().any(|s| s.address == *addr)
    }

    fn chain(&self) -> &TronChainReference {
        &self.chain_reference
    }

    async fn call_constant_a<TCalldata>(
        &self,
        contract_address: TronAddress,
        calldata: TCalldata,
        from: Option<TronAddress>,
    ) -> Result<Vec<u8>, TronChainProviderError>
    where
        TCalldata: SolCall + Send,
    {
        let url = self
            .rpc_url
            .join("wallet/triggerconstantcontract")
            .map_err(|e| TronChainProviderError::Api(e.to_string()))?;
        let body = CallConstantRequest {
            owner_address: from,
            contract_address,
            data: calldata.abi_encode(),
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

        let hex_result = resp
            .constant_result
            .first()
            .ok_or_else(|| TronChainProviderError::Api("missing constant_result".to_string()))?;

        alloy_primitives::hex::decode(hex_result)
            .map_err(|e| TronChainProviderError::AbiDecode(e.to_string()))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallConstantRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_address: Option<TronAddress>,
    pub contract_address: TronAddress,
    #[serde(with = "alloy_primitives::hex::serde")]
    pub data: Vec<u8>,
    pub call_value: u64,
    pub visible: bool,
}

#[derive(Debug, Deserialize)]
pub struct CallConstantResponse {
    pub result: TriggerStatus,
    #[serde(default)]
    pub constant_result: Vec<String>,
}
