# BCH x402 implementation plan

This is the original implementation plan and is retained as historical
context. The implementation has progressed beyond the native-only phases:
CashToken validation, P2SH20/P2SH32 destinations, wallet-facing transaction
requests, and shared fixtures are now present. See
`crates/chains/x402-chain-bch/README.md` for current usage and scope.

## Goal

Add Bitcoin Cash support to x402-rs without changing the canonical x402 v2
transport. BCH-specific behavior belongs in `x402-chain-bch`; generic x402
HTTP and facilitator plumbing should remain reusable.

## Phase 0 — freeze the wire contract

Write the BCH exact scheme specification before implementing the client or
facilitator. It must define:

- `bch:bitcoincash` and `bch:bchtest` network identities.
- Scheme name `exact` and x402 version 2.
- Native-asset spelling and `assetTransferMethod`.
- Satoshi amount encoding.
- CashAddr format and network-prefix rules.
- Signed transaction payload encoding.
- Exact payment-output semantics.
- Fee, dust, transaction-size, and input-count policies.
- Payer derivation and settlement-response semantics.
- Mempool versus confirmation requirements.
- Replay and idempotency behavior.
- `extra.assetTransferMethod = "native"` and
  `extra.paymentFlow = "upfront"` semantics.

No aliases should be accepted unless the specification explicitly defines them.
The initial contract should describe finalized raw transactions only. PSBT is
planned as a signing/interchange layer and should not be added to the x402
settlement payload until its BCH fields and finalization rules are specified.

## Phase 1 — BCH primitives

Suggested crate structure:

```text
crates/chains/x402-chain-bch/src/
  chain.rs
  address.rs
  assets.rs
  transaction.rs
  sighash.rs
  utxo.rs
  provider.rs
  psbt.rs
  v2_bch_exact/
    mod.rs
    types.rs
    client.rs
    server.rs
    facilitator.rs
```

Implement and test:

- Script-first address normalization, with CashAddr decoding to locking
  bytecode in the initial phase.
- Mainnet/chipnet prefix validation.
- Transaction encode/decode with canonical varints.
- UI/P2P TXID byte-order conversion.
- Source-output representation.
- BCH signing serialization.
- `0x41` (`SIGHASH_ALL | SIGHASH_FORKID`) signature construction and
  verification.
- P2PKH locking/unlocking scripts.
- Checked satoshi arithmetic.

Address parsing and serialization should remain separate from transaction
validation so later legacy, token-aware, or other BCH address formats can be
added without making text addresses the domain identity.

The implementation should use libauth vectors for transaction encoding,
hashing, signing serialization, and VM-compatible P2PKH behavior.

## Phase 2 — UTXO provider boundary

Define a BCH provider trait that can be implemented by BCHN RPC, Fulcrum, and
test providers. It should expose:

- Spendable-UTXO discovery.
- Source transaction/output lookup.
- Outpoint spentness.
- Mempool lookup.
- Raw transaction broadcast.
- Confirmation/status polling.
- Chain identity.

The provider must distinguish not-found, spent, conflicting, mempool, and
confirmed states. A generic network error must not be treated as proof that an
outpoint is spendable.

## Phase 3 — client-side v2 exact

Implement `V2BchExactClient` using the existing x402-rs client trait.

Client flow:

1. Parse and validate the selected requirements.
2. Select UTXOs sufficient for amount plus fee.
3. Select BCH-only UTXOs for native payments, or matching token UTXOs plus BCH
   fee inputs for CashToken payments.
4. Build the payment and change outputs.
5. Enforce dust and fee policies.
6. Sign every input with BCH replay protection.
7. Serialize the complete signed transaction.
8. Return the x402 payload containing the transaction.

The current implementation also accepts P2SH20/P2SH32 payment destinations.
That does not mean the facilitator executes arbitrary CashScript inputs or
validates future covenant successor transactions.

## Phase 4 — facilitator verification and settlement

Verification must be read-only and must prove:

- Accepted requirements exactly match the request requirements.
- Network and asset are supported.
- CashAddr resolves to the requested payment script.
- Every input has an authoritative source output.
- Every input signature is valid under BCH signing rules.
- All source inputs are unspent or otherwise valid for mempool acceptance.
- Input value covers output value and fee.
- Exactly one output pays the requested script and exact amount.
- CashToken category, amount, capability, and commitment conservation are
  checked when the requirement requests a CashToken.
- Transaction policy limits are satisfied.

Settlement must re-verify, broadcast the same raw transaction, handle already
known identical transactions idempotently, detect conflicts, and return the
TXID with `bch:*` network identity. It must apply the configured settlement
strategy: explicit mempool acceptance, provider-verified BCH double-spend
proof evidence, or a required confirmation count. A 0-conf strategy is an
explicit opt-in. If broadcast succeeded but the selected status cannot be
established, return a `settlement_pending` result with the TXID so the resource
server can reconcile instead of blindly rebroadcasting. The x402-rs paygate
must run this settlement path before resource execution for the advertised
`upfront` flow; `/verify` alone must not authorize the request.

The first server integration should use the canonical `upfront` flow. A plain
BCH transaction cannot be reserved by `/verify`, and verification alone does
not prevent a conflicting spend.

## Phase 5 — x402-rs integration and documentation

Add:

- `V2BchExact::price_tag`.
- BCH examples for Axum and Reqwest.
- Facilitator registration examples.
- `/supported` output examples.
- A BCH exact scheme specification.
- Error mapping for invalid transactions, spent inputs, conflicts, and pending
  settlement.
- Mapping from the canonical `upfront` payment flow to the x402-rs paygate
  configuration.

## Phase 6 — CashTokens (implemented)

Add a separate typed asset model in `assets.rs` for:

- Fungible token categories and amounts.
- NFT commitments.
- Immutable, mutable, and minting capabilities.
- Genesis-input rules.
- Token successor/conservation rules.
- Token-aware output prefixes.
- Token-aware address handling.

Verification proves both BCH value correctness and token-state correctness.
Genesis/provenance and arbitrary covenant successor rules remain outside x402
settlement.

## Phase 7 — address and PSBT interoperability

Add address and signing interoperability only after the native BCH path has
stable vectors and security checks:

- Support the required BCH address encodings, including legacy Base58 and
  token-aware formats where the product needs them.
- Preserve network, locking-script, and token metadata through parse/serialize
  round trips.
- Parse and serialize BCH PSBTs without dropping unknown or proprietary data.
- Define source-output and BCH sighash fields needed for offline or
  hardware-wallet signing.
- Support partial-signature exchange and deterministic finalization into the
  raw transaction consumed by x402.
- Run the same UTXO, value, asset, signature, and output checks on a raw
  transaction finalized from PSBT.

PSBT is an interoperability mechanism for wallets and signers. It does not
change the POC settlement contract: the facilitator still verifies and
broadcasts the finalized transaction, not an unfinished PSBT.

## Phase 8 — optional advanced mechanisms

Evaluate separately:

- A BCH-specific batch-settlement mechanism inspired by, but not copied from,
  `x402-bch`.
- Upto/recurring payments using covenants, vouchers, channels, or escrow.
- Facilitator fee sponsorship with an explicit signer and sighash protocol.
- General CashScript/covenant verification through a BCH VM.

These are distinct security models and should not be hidden behind the native
`exact` implementation.

## Testing requirements

### Unit and vector tests

- Network identity and address-prefix rejection.
- CashAddr/script round trips.
- Address round trips across every supported encoding, including wrong-network
  and ambiguous-prefix rejection.
- Transaction encoding and TXID vectors.
- BCH sighash vectors for `0x41`; reserve `0x61` coverage for a later
  `UTXOS`-aware phase.
- DER/public-key/signature validation.
- Satoshi overflow and fee arithmetic.
- Exact payment-output matching.
- Token-prefix rejection.
- PSBT round trips, partial-signature exchange, finalization, and preservation
  of unknown/proprietary fields.

### Negative and adversarial tests

- Wrong network.
- Wrong payee or amount.
- Duplicate payment outputs.
- Missing source output.
- Source-output substitution.
- PSBT source-output substitution and mismatched finalized-transaction checks.
- Invalid varints and trailing bytes.
- Unsupported sighash flags.
- Spent and conflicting outpoints.
- Excessive fees, inputs, outputs, or transaction size.
- Replayed identical transactions.
- Concurrent settlement attempts.

### End-to-end tests

Use a mocked provider first, then a local BCH node/chipnet harness. The full
test should exercise:

```text
402 -> client selection -> transaction build/sign
    -> PAYMENT-SIGNATURE -> verify -> settle/broadcast
    -> PAYMENT-RESPONSE
```

All successful transaction fixtures should be checked against libauth's BCH
transaction decoder and signing/VM behavior.
