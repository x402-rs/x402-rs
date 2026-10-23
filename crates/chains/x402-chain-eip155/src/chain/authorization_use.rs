//! Finding on chain the transaction that used an ERC-3009 authorization.
//!
//! A settle request whose payment was already settled has nothing left to send: its
//! authorization is used, and settling it again fails. When the pending-settlement store has no
//! entry for such a request (the facilitator restarted, or the retry reached another instance),
//! the settlement can still be found on chain: `transferWithAuthorization` emits
//! `AuthorizationUsed(authorizer, nonce)` with both arguments indexed, and the contract only
//! accepts the authorization in a block whose timestamp is after `validAfter` and before
//! `validBefore`, so a bounded `eth_getLogs` on the asset finds the transaction.
//!
//! [`query_authorization_use`] finds the transaction and [`settlement_of_authorization`] checks,
//! on its receipt, that it is the settlement the request asks for. The search is clamped to what
//! the RPC answers: it starts no earlier than the lookback before the latest block, whatever the
//! authorization's validity window, each `eth_getLogs` call covers at most the configured number
//! of blocks, and a range the RPC refuses is split until it is accepted. Permit2 payments are
//! not covered: their nonces live in the Permit2 contract's bitmap and need their own lookup.

use alloy_primitives::{Address, B256, TxHash, U256};
use alloy_provider::Provider;
use alloy_rpc_types_eth::{BlockNumberOrTag, Filter, TransactionReceipt};
use alloy_sol_types::SolEvent;
use alloy_transport::{RpcError, TransportError};
use std::num::NonZeroU64;
use std::time::Duration;
use x402_types::timestamp::UnixTimestamp;

use crate::chain::pending_settlement::PENDING_SETTLEMENT_TTL;
use crate::v1_eip155_exact::{ExactEvmPayloadAuthorization, IEIP3009};

/// How far back, from the latest block, a used authorization is looked for: as long as a
/// pending-settlement entry is kept, so a settlement is not answered from the chain for longer
/// than a store would have kept it, and the logs searched stay a few minutes of blocks.
pub const AUTHORIZATION_USE_LOOKBACK: Duration = PENDING_SETTLEMENT_TTL;

/// Most blocks one `eth_getLogs` call covers unless the chain's configuration sets
/// `log_query_max_blocks`. Longer ranges are split, newest first. Public endpoints cap the
/// range of a log query (`https://sepolia.base.org` at 201 blocks), and on a chain with
/// sub-second blocks the lookback spans more than that.
pub const LOG_QUERY_MAX_BLOCKS: NonZeroU64 = NonZeroU64::new(100).unwrap();

/// Most `eth_getLogs` calls one lookup makes, refused ones included: at most the newest
/// `LOG_QUERY_MAX_CALLS` times the blocks per call of the range are searched.
pub const LOG_QUERY_MAX_CALLS: usize = 50;

/// Blocks to step back from the latest block for the first timestamp sample.
const FIRST_STEP_BLOCKS: u64 = 16;

/// The ERC-3009 authorization of a settle request, with the asset it was signed for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Eip3009Authorization {
    /// The token contract the authorization was signed for.
    pub asset: Address,
    /// The payer, who signed the authorization (`authorizer` in `AuthorizationUsed`).
    pub from: Address,
    /// The recipient.
    pub to: Address,
    /// The amount the authorization transfers.
    pub value: U256,
    /// The authorization is valid in blocks with a timestamp after this.
    pub valid_after: UnixTimestamp,
    /// The authorization is valid in blocks with a timestamp before this.
    pub valid_before: UnixTimestamp,
    /// The authorization's nonce.
    pub nonce: B256,
}

impl Eip3009Authorization {
    /// The authorization of a payment payload, for `asset`.
    pub fn new(asset: Address, authorization: &ExactEvmPayloadAuthorization) -> Self {
        Self {
            asset,
            from: authorization.from,
            to: authorization.to,
            value: authorization.value,
            valid_after: authorization.valid_after,
            valid_before: authorization.valid_before,
            nonce: authorization.nonce,
        }
    }
}

/// The transaction found to have used an authorization.
#[derive(Debug, Clone)]
pub struct AuthorizationUse {
    /// Its hash, from the `AuthorizationUsed` log.
    pub tx_hash: TxHash,
    /// Its receipt, or `None` if the receipt could not be fetched in time.
    pub receipt: Option<TransactionReceipt>,
}

/// Why a lookup of a used authorization stopped.
#[derive(Debug, thiserror::Error)]
pub enum AuthorizationUseLookupError {
    /// Reading the authorization's state from the token contract failed.
    #[error("authorizationState call failed: {0}")]
    Contract(#[from] alloy_contract::Error),
    /// An RPC call failed.
    #[error(transparent)]
    Transport(#[from] TransportError),
    /// The RPC did not return a block it was asked for.
    #[error("block {0} not found")]
    MissingBlock(BlockNumberOrTag),
    /// The lookup made [`LOG_QUERY_MAX_CALLS`] `eth_getLogs` calls without finding the use, and
    /// the older blocks of the range were not searched.
    #[error(
        "no use found after {LOG_QUERY_MAX_CALLS} eth_getLogs calls; blocks {from}..={to} not searched"
    )]
    CallLimit {
        /// The oldest block of the range.
        from: u64,
        /// The newest block not searched.
        to: u64,
    },
}

/// Looks for the transaction that used `authorization`, and returns its hash.
///
/// `None` if the authorization is not used, or if it was not used in the last `lookback` before
/// the latest block. Reads, in order: `authorizationState` on the asset (one call; nothing more
/// when the authorization is unused), the latest block, a few block headers to find where the
/// range starts, and `eth_getLogs` for `AuthorizationUsed(from, nonce)` on the asset over the
/// blocks that can hold the use: at most `max_blocks_per_query` per call, newest first, and at
/// most [`LOG_QUERY_MAX_CALLS`] calls. When the RPC refuses the block range of a call, the
/// newer half of that range is queried instead, and later calls keep the smaller size; a
/// refused single block, or any other error, ends the lookup with that error. Running out of
/// calls before the oldest block ends it with [`AuthorizationUseLookupError::CallLimit`].
pub async fn query_authorization_use<P: Provider>(
    provider: &P,
    authorization: &Eip3009Authorization,
    lookback: Duration,
    max_blocks_per_query: NonZeroU64,
) -> Result<Option<TxHash>, AuthorizationUseLookupError> {
    let used = IEIP3009::new(authorization.asset, provider)
        .authorizationState(authorization.from, authorization.nonce)
        .call()
        .await?;
    if !used {
        return Ok(None);
    }

    let latest = block_header(provider, BlockNumberOrTag::Latest).await?;
    // The use is in a block with a timestamp after `validAfter`, before `validBefore`, and
    // (by choice) within the lookback. The range ends at the latest block, so a far
    // `validBefore` does not widen it, and an early `validAfter` is cut at the lookback.
    let lowest_timestamp = authorization
        .valid_after
        .as_secs()
        .max(latest.timestamp.saturating_sub(lookback.as_secs()));
    if authorization.valid_before.as_secs() <= lowest_timestamp {
        return Ok(None);
    }
    let from_block = block_at_or_before(
        latest.number,
        latest.timestamp,
        lowest_timestamp,
        |number| async move {
            block_header(provider, BlockNumberOrTag::Number(number))
                .await
                .map(|header| header.timestamp)
        },
    )
    .await?;

    let filter = Filter::new()
        .address(authorization.asset)
        .event_signature(IEIP3009::AuthorizationUsed::SIGNATURE_HASH)
        .topic1(authorization.from.into_word())
        .topic2(authorization.nonce);
    // Newest first; `end` is the newest block not searched yet.
    let mut blocks_per_query = max_blocks_per_query.get();
    let mut end = latest.number;
    for _ in 0..LOG_QUERY_MAX_CALLS {
        let start = end.saturating_sub(blocks_per_query - 1).max(from_block);
        let logs = match provider
            .get_logs(&filter.clone().from_block(start).to_block(end))
            .await
        {
            Ok(logs) => logs,
            Err(error) if start < end && is_range_refusal(&error) => {
                let refused_blocks = end - start + 1;
                blocks_per_query = refused_blocks / 2;
                continue;
            }
            Err(error) => return Err(error.into()),
        };
        // At most one log matches: an authorization is used once.
        if let Some(tx_hash) = logs.iter().find_map(|log| log.transaction_hash) {
            return Ok(Some(tx_hash));
        }
        if start == from_block {
            return Ok(None);
        }
        end = start - 1;
    }
    Err(AuthorizationUseLookupError::CallLimit {
        from: from_block,
        to: end,
    })
}

/// Whether `error` is the RPC refusing the block range of an `eth_getLogs` call. RPCs refuse it
/// with different codes and HTTP statuses, and their messages name the range:
/// `https://sepolia.base.org` answers HTTP 413 with `-32614 eth_getLogs is limited to a 200
/// range`, publicnode answers HTTP 200 with `-32701 exceed maximum block range: 50000`. Other
/// errors, a rate limit among them (`https://mainnet.base.org`: HTTP 429 with `-32011 request
/// limit reached`), are not retried. A cap on the number of results does not arise here: the
/// filter has both indexed topics, so at most one log matches.
fn is_range_refusal(error: &TransportError) -> bool {
    let names_the_range = |message: &str| message.to_ascii_lowercase().contains("range");
    match error {
        RpcError::ErrorResp(payload) => names_the_range(&payload.message),
        // With an HTTP error status, the JSON-RPC error is in the body.
        RpcError::Transport(kind) => kind.as_http_error().is_some_and(|http| {
            serde_json::from_str::<serde_json::Value>(&http.body).is_ok_and(|body| {
                body.pointer("/error/message")
                    .and_then(serde_json::Value::as_str)
                    .is_some_and(names_the_range)
            })
        }),
        _ => false,
    }
}

/// The settlement `receipt` holds for `authorization`, if it is one: a successful transaction
/// sent by one of `signers` (this facilitator's), in which the asset emitted
/// `AuthorizationUsed(from, nonce)` and a `Transfer` of the authorized value from the payer to
/// the recipient.
pub fn settlement_of_authorization(
    receipt: &TransactionReceipt,
    authorization: &Eip3009Authorization,
    signers: &[Address],
) -> Result<TxHash, AuthorizationUseMismatch> {
    if !receipt.status() {
        return Err(AuthorizationUseMismatch::Reverted);
    }
    if !signers.contains(&receipt.from) {
        return Err(AuthorizationUseMismatch::OtherSender);
    }
    let asset_logs = || {
        receipt
            .logs()
            .iter()
            .filter(|log| log.address() == authorization.asset)
    };
    let used = [
        IEIP3009::AuthorizationUsed::SIGNATURE_HASH,
        authorization.from.into_word(),
        authorization.nonce,
    ];
    if !asset_logs().any(|log| log.topics() == used.as_slice()) {
        return Err(AuthorizationUseMismatch::AuthorizationNotUsed);
    }
    let transferred = asset_logs().any(|log| {
        log.log_decode::<IEIP3009::Transfer>()
            .is_ok_and(|transfer| {
                let transfer = transfer.inner.data;
                transfer.from == authorization.from
                    && transfer.to == authorization.to
                    && transfer.value == authorization.value
            })
    });
    if !transferred {
        return Err(AuthorizationUseMismatch::TransferMismatch);
    }
    Ok(receipt.transaction_hash)
}

/// Why a transaction found for a used authorization is not taken as its settlement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AuthorizationUseMismatch {
    /// The transaction reverted.
    #[error("the transaction reverted")]
    Reverted,
    /// The transaction was not sent by any of this facilitator's signers.
    #[error("the transaction was not sent by this facilitator")]
    OtherSender,
    /// The asset emitted no `AuthorizationUsed` for the authorization in the transaction.
    #[error("the transaction did not use the authorization")]
    AuthorizationNotUsed,
    /// The asset emitted no `Transfer` of the authorized value from the payer to the recipient.
    #[error(
        "the transaction has no transfer of the authorized value from the payer to the recipient"
    )]
    TransferMismatch,
}

struct BlockHeader {
    number: u64,
    timestamp: u64,
}

async fn block_header<P: Provider>(
    provider: &P,
    block: BlockNumberOrTag,
) -> Result<BlockHeader, AuthorizationUseLookupError> {
    let header = provider
        .get_block_by_number(block)
        .await?
        .ok_or(AuthorizationUseLookupError::MissingBlock(block))?
        .header;
    Ok(BlockHeader {
        number: header.number,
        timestamp: header.timestamp,
    })
}

/// The number of a block with a timestamp at or before `target`, found by stepping back from
/// the latest block. Every block after `target` comes after the returned one, so a range from it
/// to the latest block holds them all. Each step is scaled by the block rate seen so far, with a
/// margin, and at least doubles, so a few `timestamp_of` lookups reach it whatever the chain's
/// block time. Returns `latest` if it is not after `target`, and 0 if every block is.
async fn block_at_or_before<F, Fut, E>(
    latest: u64,
    latest_timestamp: u64,
    target: u64,
    mut timestamp_of: F,
) -> Result<u64, E>
where
    F: FnMut(u64) -> Fut,
    Fut: Future<Output = Result<u64, E>>,
{
    if latest_timestamp <= target {
        return Ok(latest);
    }
    let needed = latest_timestamp - target;
    let mut distance = FIRST_STEP_BLOCKS;
    loop {
        let number = latest.saturating_sub(distance);
        let timestamp = timestamp_of(number).await?;
        if timestamp <= target || number == 0 {
            return Ok(number);
        }
        let covered = latest_timestamp.saturating_sub(timestamp).max(1);
        let estimate = distance
            .saturating_mul(needed)
            .div_ceil(covered)
            .saturating_mul(5)
            / 4
            + 1;
        distance = estimate.max(distance.saturating_mul(2));
    }
}

#[cfg(test)]
pub(crate) mod tests {
    //! Also used by the provider's tests: the lookup's inputs and the JSON-RPC answers it reads.
    use super::*;
    use alloy_primitives::{address, b256};
    use alloy_provider::RootProvider;
    use alloy_rpc_client::RpcClient;
    use alloy_transport::mock::{Asserter, MockResponse, MockTransport};
    use serde::Serialize;
    use serde_json::{Value, json};
    use std::pin::Pin;
    use std::sync::{Arc, Mutex};
    use std::task::{Context, Poll};

    pub(crate) const ASSET: Address = address!("0x036CbD53842c5426634e7929541eC2318f3dCF7e");
    pub(crate) const PAYER: Address = address!("0x1111111111111111111111111111111111111111");
    pub(crate) const SELLER: Address = address!("0x2222222222222222222222222222222222222222");
    pub(crate) const NONCE: B256 =
        b256!("0x4444444444444444444444444444444444444444444444444444444444444444");
    pub(crate) const TX: TxHash =
        b256!("0x5555555555555555555555555555555555555555555555555555555555555555");

    pub(crate) fn authorization(valid_after: u64, valid_before: u64) -> Eip3009Authorization {
        Eip3009Authorization {
            asset: ASSET,
            from: PAYER,
            to: SELLER,
            value: U256::from(10_000u64),
            valid_after: UnixTimestamp::from_secs(valid_after),
            valid_before: UnixTimestamp::from_secs(valid_before),
            nonce: NONCE,
        }
    }

    // ---- block_at_or_before ----

    /// Timestamp of block `number` on a chain with one block every `seconds_per_block`, from
    /// block 0 at time 0.
    fn timestamp_at(number: u64, seconds_per_block: f64) -> u64 {
        (number as f64 * seconds_per_block) as u64
    }

    async fn search(latest: u64, seconds_per_block: f64, target: u64) -> (u64, u32) {
        let lookups = std::sync::atomic::AtomicU32::new(0);
        let found = block_at_or_before(
            latest,
            timestamp_at(latest, seconds_per_block),
            target,
            |number| {
                lookups.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                async move { Ok::<_, ()>(timestamp_at(number, seconds_per_block)) }
            },
        )
        .await
        .unwrap();
        (found, lookups.into_inner())
    }

    #[tokio::test]
    async fn test_block_search_lands_at_or_before_the_target_in_few_lookups() {
        // Two-second blocks (Base), quarter-second blocks (Arbitrum), twelve-second blocks
        // (Ethereum): five minutes back from block 30,000,000.
        for seconds_per_block in [2.0, 0.25, 12.0] {
            let latest = 30_000_000;
            let target = timestamp_at(latest, seconds_per_block) - 300;
            let (found, lookups) = search(latest, seconds_per_block, target).await;
            assert!(timestamp_at(found, seconds_per_block) <= target);
            // Not much further back than needed: the range stays about five minutes long.
            let needed_blocks = (300.0 / seconds_per_block) as u64;
            assert!(
                latest - found <= 2 * needed_blocks + FIRST_STEP_BLOCKS,
                "{seconds_per_block} s blocks: {} blocks back",
                latest - found
            );
            assert!(
                lookups <= 4,
                "{seconds_per_block} s blocks: {lookups} lookups"
            );
        }
    }

    #[tokio::test]
    async fn test_block_search_returns_latest_when_it_is_not_after_the_target() {
        assert_eq!(search(1_000, 2.0, 5_000).await, (1_000, 0));
    }

    #[tokio::test]
    async fn test_block_search_stops_at_genesis() {
        let (found, _) = search(100, 2.0, 0).await;
        assert_eq!(found, 0);
    }

    #[tokio::test]
    async fn test_block_search_passes_lookup_errors_on() {
        let result =
            block_at_or_before(100, 200, 50, |_| async { Err::<u64, _>("HTTP error 503") }).await;
        assert_eq!(result, Err("HTTP error 503"));
    }

    // ---- query_authorization_use against a mock RPC ----

    /// An error the RPC answers to a request, instead of the mock transport, or `None`.
    type Refusal = Arc<dyn Fn(&Value) -> Option<TransportError> + Send + Sync>;

    /// Keeps each request, as JSON, for the test to read, answers the ones `refuse` refuses with
    /// its error, and passes the others to the mock transport.
    #[derive(Clone)]
    struct Recorded<S> {
        inner: S,
        requests: Arc<Mutex<Vec<Value>>>,
        refuse: Refusal,
    }

    impl<S, R> tower::Service<R> for Recorded<S>
    where
        S: tower::Service<R, Error = TransportError>,
        S::Response: Send + 'static,
        S::Future: Send + 'static,
        R: Serialize,
    {
        type Response = S::Response;
        type Error = TransportError;
        type Future = Pin<Box<dyn Future<Output = Result<S::Response, TransportError>> + Send>>;

        fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
            self.inner.poll_ready(cx)
        }

        fn call(&mut self, request: R) -> Self::Future {
            let json = serde_json::to_value(&request).unwrap();
            let refusal = (self.refuse)(&json);
            self.requests.lock().unwrap().push(json);
            match refusal {
                Some(error) => Box::pin(std::future::ready(Err(error))),
                None => Box::pin(self.inner.call(request)),
            }
        }
    }

    fn mock_rpc() -> (RootProvider, Asserter, Arc<Mutex<Vec<Value>>>) {
        mock_rpc_refusing(|_| None)
    }

    fn mock_rpc_refusing(
        refuse: impl Fn(&Value) -> Option<TransportError> + Send + Sync + 'static,
    ) -> (RootProvider, Asserter, Arc<Mutex<Vec<Value>>>) {
        let asserter = Asserter::new();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let transport = Recorded {
            inner: MockTransport::new(asserter.clone()),
            requests: requests.clone(),
            refuse: Arc::new(refuse),
        };
        let provider = RootProvider::new(RpcClient::new(transport, true));
        (provider, asserter, requests)
    }

    fn block_number(value: &Value) -> u64 {
        u64::from_str_radix(value.as_str().unwrap().trim_start_matches("0x"), 16).unwrap()
    }

    /// `https://sepolia.base.org`'s answer to an `eth_getLogs` over more than 201 blocks
    /// (`toBlock - fromBlock` above 200).
    fn base_sepolia_log_range_cap(request: &Value) -> Option<TransportError> {
        let filter = &request["params"][0];
        let refused = request["method"] == "eth_getLogs"
            && block_number(&filter["toBlock"]) - block_number(&filter["fromBlock"]) > 200;
        refused.then(|| {
            alloy_transport::TransportErrorKind::http_error(
                413,
                json!({
                    "jsonrpc": "2.0",
                    "id": request["id"],
                    "error": {"code": -32614, "message": "eth_getLogs is limited to a 200 range"}
                })
                .to_string(),
            )
        })
    }

    fn methods(requests: &Mutex<Vec<Value>>) -> Vec<String> {
        requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request["method"].as_str().unwrap().to_string())
            .collect()
    }

    /// The `fromBlock..=toBlock` of each `eth_getLogs` request, in order.
    fn log_ranges(requests: &Mutex<Vec<Value>>) -> Vec<(u64, u64)> {
        requests
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request["method"] == "eth_getLogs")
            .map(|request| {
                let filter = &request["params"][0];
                (
                    block_number(&filter["fromBlock"]),
                    block_number(&filter["toBlock"]),
                )
            })
            .collect()
    }

    /// Pushes a JSON-RPC error answer.
    fn push_rpc_error(asserter: &Asserter, code: i64, message: &str) {
        asserter.push(MockResponse::Failure(
            serde_json::from_value(json!({ "code": code, "message": message })).unwrap(),
        ));
    }

    /// The answers up to the first `eth_getLogs` for a used authorization on a chain with a
    /// block every 0.2 s (Base Sepolia): the latest block is 10,000 at time 2,000, and the five
    /// minutes back reach block 8,500, so the range is 8,499..=10,000, 1,502 blocks.
    fn push_used_on_a_fast_chain(asserter: &Asserter) {
        asserter.push_success(&abi_bool(true));
        asserter.push_success(&block(10_000, 2_000));
        asserter.push_success(&block(9_984, 1_996));
        asserter.push_success(&block(8_499, 1_699));
    }

    pub(crate) fn abi_bool(value: bool) -> String {
        format!("0x{:064x}", u8::from(value))
    }

    pub(crate) fn block(number: u64, timestamp: u64) -> Value {
        json!({
            "hash": B256::with_last_byte(1),
            "parentHash": B256::ZERO,
            "sha3Uncles": B256::ZERO,
            "miner": Address::ZERO,
            "stateRoot": B256::ZERO,
            "transactionsRoot": B256::ZERO,
            "receiptsRoot": B256::ZERO,
            "logsBloom": format!("0x{}", "0".repeat(512)),
            "difficulty": "0x0",
            "number": format!("0x{number:x}"),
            "gasLimit": "0x1",
            "gasUsed": "0x0",
            "timestamp": format!("0x{timestamp:x}"),
            "extraData": "0x",
            "mixHash": B256::ZERO,
            "nonce": "0x0000000000000000",
            "baseFeePerGas": "0x1",
            "uncles": [],
            "transactions": []
        })
    }

    pub(crate) fn authorization_used_log(block_number: u64) -> Value {
        json!({
            "address": ASSET,
            "topics": [
                IEIP3009::AuthorizationUsed::SIGNATURE_HASH,
                PAYER.into_word(),
                NONCE
            ],
            "data": "0x",
            "blockHash": B256::with_last_byte(2),
            "blockNumber": format!("0x{block_number:x}"),
            "transactionHash": TX,
            "transactionIndex": "0x0",
            "logIndex": "0x0",
            "removed": false
        })
    }

    #[tokio::test]
    async fn test_unused_authorization_is_one_call() {
        let (provider, asserter, requests) = mock_rpc();
        asserter.push_success(&abi_bool(false));
        let found = query_authorization_use(
            &provider,
            &authorization(0, u64::MAX),
            AUTHORIZATION_USE_LOOKBACK,
            LOG_QUERY_MAX_BLOCKS,
        )
        .await
        .unwrap();
        assert!(found.is_none());
        assert_eq!(methods(&requests), vec!["eth_call"]);
    }

    #[tokio::test]
    async fn test_used_authorization_is_found_with_a_bounded_log_query() {
        let (provider, asserter, requests) = mock_rpc();
        // Two-second blocks; the latest is block 10,000 at time 20,000.
        asserter.push_success(&abi_bool(true));
        asserter.push_success(&block(10_000, 20_000));
        // Stepping back: block 9,984 (time 19,968) is still inside the lookback (300 s, back to
        // time 19,700), and block 9,812 (time 19,624), 188 blocks back, is before it.
        asserter.push_success(&block(9_984, 19_968));
        asserter.push_success(&block(9_812, 19_624));
        asserter.push_success(&vec![authorization_used_log(9_990)]);

        let found = query_authorization_use(
            &provider,
            &authorization(19_500, 20_300),
            AUTHORIZATION_USE_LOOKBACK,
            LOG_QUERY_MAX_BLOCKS,
        )
        .await
        .unwrap();
        assert_eq!(found, Some(TX));

        assert_eq!(
            methods(&requests),
            vec![
                "eth_call",
                "eth_getBlockByNumber",
                "eth_getBlockByNumber",
                "eth_getBlockByNumber",
                "eth_getLogs"
            ]
        );
        let requests = requests.lock().unwrap();
        assert_eq!(requests[1]["params"][0], json!("latest"));
        assert_eq!(requests[2]["params"][0], json!("0x2700")); // 9,984
        assert_eq!(requests[3]["params"][0], json!("0x2654")); // 9,812
        let filter = &requests[4]["params"][0];
        assert_eq!(filter["address"], json!(ASSET));
        assert_eq!(
            filter["topics"],
            json!([
                IEIP3009::AuthorizationUsed::SIGNATURE_HASH,
                PAYER.into_word(),
                NONCE
            ])
        );
        // The newest 100 blocks of the range 9,812..=10,000 first; the log is there.
        assert_eq!(filter["fromBlock"], json!("0x26ad")); // 9,901
        assert_eq!(filter["toBlock"], json!("0x2710")); // 10,000
    }

    #[tokio::test]
    async fn test_used_authorization_without_a_log_in_range_is_not_found() {
        let (provider, asserter, requests) = mock_rpc();
        asserter.push_success(&abi_bool(true));
        asserter.push_success(&block(10_000, 20_000));
        asserter.push_success(&block(9_984, 19_968));
        asserter.push_success(&block(9_812, 19_624));
        asserter.push_success(&Vec::<Value>::new());
        asserter.push_success(&Vec::<Value>::new());
        let found = query_authorization_use(
            &provider,
            &authorization(19_500, 20_300),
            AUTHORIZATION_USE_LOOKBACK,
            LOG_QUERY_MAX_BLOCKS,
        )
        .await
        .unwrap();
        assert!(found.is_none());
        let requests = requests.lock().unwrap();
        let ranges: Vec<_> = requests
            .iter()
            .filter(|request| request["method"] == "eth_getLogs")
            .map(|request| {
                let filter = &request["params"][0];
                (filter["fromBlock"].clone(), filter["toBlock"].clone())
            })
            .collect();
        assert_eq!(
            ranges,
            vec![
                (json!("0x26ad"), json!("0x2710")), // 9,901..=10,000
                (json!("0x2654"), json!("0x26ac")), // 9,812..=9,900
            ]
        );
    }

    #[tokio::test]
    async fn test_authorization_that_expired_before_the_lookback_is_not_searched() {
        let (provider, asserter, requests) = mock_rpc();
        asserter.push_success(&abi_bool(true));
        asserter.push_success(&block(10_000, 20_000));
        // validBefore 19,600 is more than 300 s before the latest block: any use is older.
        let found = query_authorization_use(
            &provider,
            &authorization(19_000, 19_600),
            AUTHORIZATION_USE_LOOKBACK,
            LOG_QUERY_MAX_BLOCKS,
        )
        .await
        .unwrap();
        assert!(found.is_none());
        assert_eq!(methods(&requests), vec!["eth_call", "eth_getBlockByNumber"]);
    }

    #[tokio::test]
    async fn test_refused_range_is_halved_until_the_rpc_takes_it() {
        // Configured for 1,000 blocks per call, against an RPC that takes at most 201.
        let (provider, asserter, requests) = mock_rpc_refusing(base_sepolia_log_range_cap);
        push_used_on_a_fast_chain(&asserter);
        asserter.push_success(&Vec::<Value>::new());
        asserter.push_success(&Vec::<Value>::new());
        asserter.push_success(&vec![authorization_used_log(9_700)]);
        let found = query_authorization_use(
            &provider,
            &authorization(1_000, 2_300),
            AUTHORIZATION_USE_LOOKBACK,
            NonZeroU64::new(1_000).unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(found, Some(TX));
        assert_eq!(
            log_ranges(&requests),
            vec![
                (9_001, 10_000), // refused
                (9_501, 10_000), // refused
                (9_751, 10_000), // refused
                (9_876, 10_000),
                // The rest of the range keeps the size the RPC took.
                (9_751, 9_875),
                (9_626, 9_750),
            ]
        );
    }

    #[tokio::test]
    async fn test_refused_single_block_ends_the_lookup_with_the_error() {
        let (provider, asserter, requests) = mock_rpc();
        push_used_on_a_fast_chain(&asserter);
        for _ in 0..7 {
            push_rpc_error(&asserter, -32602, "block range too large");
        }
        let result = query_authorization_use(
            &provider,
            &authorization(1_000, 2_300),
            AUTHORIZATION_USE_LOOKBACK,
            LOG_QUERY_MAX_BLOCKS,
        )
        .await;
        assert!(matches!(
            result,
            Err(AuthorizationUseLookupError::Transport(_))
        ));
        let sizes: Vec<u64> = log_ranges(&requests)
            .iter()
            .map(|(from, to)| to - from + 1)
            .collect();
        assert_eq!(sizes, vec![100, 50, 25, 12, 6, 3, 1]);
        assert!(asserter.read_q().is_empty());
    }

    #[tokio::test]
    async fn test_other_log_query_errors_are_not_retried() {
        // `https://mainnet.base.org` rate-limiting.
        let (provider, asserter, requests) = mock_rpc_refusing(|request| {
            (request["method"] == "eth_getLogs").then(|| {
                alloy_transport::TransportErrorKind::http_error(
                    429,
                    json!({
                        "jsonrpc": "2.0",
                        "id": request["id"],
                        "error": {"code": -32011, "message": "request limit reached"}
                    })
                    .to_string(),
                )
            })
        });
        push_used_on_a_fast_chain(&asserter);
        let result = query_authorization_use(
            &provider,
            &authorization(1_000, 2_300),
            AUTHORIZATION_USE_LOOKBACK,
            LOG_QUERY_MAX_BLOCKS,
        )
        .await;
        assert!(matches!(
            result,
            Err(AuthorizationUseLookupError::Transport(_))
        ));
        assert_eq!(log_ranges(&requests), vec![(9_901, 10_000)]);
    }

    #[tokio::test]
    async fn test_lookup_stops_at_the_call_limit_with_the_newest_blocks_searched() {
        let (provider, asserter, requests) = mock_rpc();
        push_used_on_a_fast_chain(&asserter);
        // 10 blocks per call: 50 calls cover 500 of the 1,502 blocks.
        for _ in 0..LOG_QUERY_MAX_CALLS {
            asserter.push_success(&Vec::<Value>::new());
        }
        let result = query_authorization_use(
            &provider,
            &authorization(1_000, 2_300),
            AUTHORIZATION_USE_LOOKBACK,
            NonZeroU64::new(10).unwrap(),
        )
        .await;
        assert!(matches!(
            result,
            Err(AuthorizationUseLookupError::CallLimit {
                from: 8_499,
                to: 9_500
            })
        ));
        let ranges = log_ranges(&requests);
        assert_eq!(ranges.len(), LOG_QUERY_MAX_CALLS);
        assert_eq!(ranges.first(), Some(&(9_991, 10_000)));
        assert_eq!(ranges.last(), Some(&(9_501, 9_510)));
    }

    #[tokio::test]
    async fn test_rpc_errors_end_the_lookup_with_an_error() {
        let (provider, asserter, _) = mock_rpc();
        asserter.push_success(&abi_bool(true));
        asserter.push_failure_msg("no backend is currently healthy to serve traffic");
        let result = query_authorization_use(
            &provider,
            &authorization(0, u64::MAX),
            AUTHORIZATION_USE_LOOKBACK,
            LOG_QUERY_MAX_BLOCKS,
        )
        .await;
        assert!(matches!(
            result,
            Err(AuthorizationUseLookupError::Transport(_))
        ));
    }
}
