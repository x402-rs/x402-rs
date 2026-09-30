# x402-chain-bch

Bitcoin Cash exact payment support for x402 v2.

The BCH chain IDs exposed by this crate are:

| Network | Chain ID |
| --- | --- |
| Mainnet | `bch:bitcoincash` |
| Chipnet | `bch:bchtest` |

The references are the CashAddr network prefixes used by BCH integrations.
They are not derived from the Bitcoin/BCH genesis block or fork height.

The client submits a complete signed BCH transaction. The facilitator fetches
authoritative source outputs, validates the BCH transition, and broadcasts the
same transaction. BCH `SIGHASH_ALL | SIGHASH_FORKID` (`0x41`) is used for the
standard P2PKH path. The implementation supports native BCH, fungible and NFT
CashTokens, and P2PKH/P2SH20/P2SH32 payment outputs. CashScript is supported as
an output/locking-script destination: x402 does not execute arbitrary
CashScript spending conditions or act as a covenant engine.

The wallet is responsible for selecting and reserving UTXOs, adding BCH and
token change, deriving change addresses, signing every input, and returning a
finalized raw transaction. The facilitator must not append inputs or mutate a
signed transaction. BCH amounts are satoshis; token amounts are atomic units.
Both are conserved independently.

The wallet-facing request uses `mainnet` or `chipnet`. The x402 wire network
identities remain `bch:bitcoincash` and `bch:bchtest`.

`FulcrumProvider` implements the Electrum Cash JSON-RPC boundary used by
Fulcrum. Applications may supply a TLS or WebSocket transport through the
`FulcrumTransport` trait and should inject a shared `BchSettlementStore` for
multi-process facilitator deployments.

The adapter accounts for Fulcrum's two amount encodings: verbose transaction
outputs are BCH decimal values, while blockchain.scripthash.listunspent returns
integer satoshis.

The crate test suite includes `test/fixtures/bch-exact-p2pkh.json`, a
deterministic native-BCH P2PKH payment fixture shared with the TypeScript
`@x402/bch` implementation. It covers the serialized transaction, source
output, merchant amount, payer, transaction ID, and fee so integrations can
compare results across both SDKs.

## Transaction lifecycle

```text
requirements -> wallet UTXO selection -> build outputs/change -> sign
             -> x402 payload -> facilitator source-output validation
             -> broadcast -> mempool/confirmation reconciliation
```

Inputs contain outpoints, not the previous output value or locking script.
Consequently, the facilitator provider is authoritative for source outputs;
client-supplied source data cannot replace that lookup. A transport failure
after broadcast is indeterminate and must be reconciled by TXID. It must not
be treated as permission to build a second spend.

## CashTokens and P2SH32

Native requirements use `asset: "BCH"` and
`extra.assetTransferMethod: "native"`. CashToken requirements use
`extra.assetTransferMethod: "cashtoken"`, a token category and atomic amount,
and optional NFT capability/commitment data. Fungible token amounts, NFT
commitments/capabilities, BCH output values, and token change are validated as
separate UTXO invariants.

P2SH32 is a valid payment destination for compiled CashScript contracts. The
facilitator verifies that the requested output pays the requested locking
script, but it does not prove the contract's future successor transaction.
Consensus validity, covenant topology, token provenance, wallet
authorization, and x402 payment settlement are separate concerns.

## Electrum endpoint redundancy

`FulcrumProvider` accepts an injected `FulcrumTransport`; the crate does not
silently select or trust a public server. For live deployments, applications
should configure a failover transport with more than one endpoint and prefer
TLS (port `50002`) or WSS (port `50004`) where available. The following set is
the BCH Electrum set referenced by CashScript's network-provider sources and
migration notes:

| Network | Endpoints |
| --- | --- |
| Mainnet | `bch.imaginary.cash`, `blackie.c3-soft.com`, `electroncash.dk` |
| Chipnet | `chipnet.bch.ninja` |

`FailoverFulcrumTransport` provides the minimum sequential failover behavior:
pass it caller-created transports in the desired order. It retries all
requests, including broadcasts; if a broadcast response is lost after the
server accepts a transaction, applications must reconcile the result by
checking transaction status rather than assuming the broadcast failed.

```rust,ignore
let transport = FailoverFulcrumTransport::new(vec![primary, secondary])?;
let provider = FulcrumProvider::new(transport, BchChainReference::MAINNET);
```

The bundled `FulcrumTcpTransport` is intentionally a plain TCP building block;
applications requiring TLS or WSS should provide a transport implementation
that performs certificate validation. Availability redundancy is not chain
verification: applications should compare chain tip/header data across
independent servers when making operational decisions.

Endpoint availability and chain consistency are deployment concerns and should
be revalidated by each operator. Do not disable certificate validation for a
failover endpoint.

```rust,ignore
use x402_chain_bch::{BchChainReference, FulcrumProvider, FulcrumTcpTransport, V2BchExact};
use x402_types::scheme::X402SchemeFacilitatorBuilder;

let transport = FulcrumTcpTransport::connect("127.0.0.1:50002").await?;
let provider = FulcrumProvider::new(transport, BchChainReference::CHIPNET);
let facilitator = V2BchExact.build(provider, None)?;
```
