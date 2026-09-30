# BCH x402 implementation readiness assessment

This record describes the current implementation, not the earlier native-only
POC boundary. The implementation has since added CashToken and P2SH32
validation, wallet-facing transaction requests, HD-wallet utilities, and
shared TypeScript/Rust fixtures.

## Verdict

The implementation is suitable for controlled server and wallet-adapter
testing. It is not a turnkey end-user wallet: applications still provide the
wallet transport, UTXO reservation policy, and operational provider setup.
Those are wallet/application responsibilities rather than missing BCH protocol
features.

The two SDK implementations do not need identical internal APIs. They must
produce and consume the same x402 v2 wire representation and the same BCH
transaction validity rules.

## Supported first release boundary

The first release can be reliable if it explicitly supports only:

- x402 v2 `exact`.
- `bch:bitcoincash` and `bch:bchtest`.
- Native BCH, represented as `asset: "BCH"` and
  `extra.assetTransferMethod: "native"`.
- `extra.paymentFlow: "upfront"`.
- CashAddr recipients with strict network validation.
- Standard P2PKH inputs, multiple inputs, one exact merchant output, and
  ordinary BCH change outputs.
- A complete client-signed raw transaction carried as
  `payload.transaction`.
- Client-paid fees; no facilitator input insertion, transaction mutation, or
  fee sponsorship.
- Fungible and NFT CashToken conservation, including NFT capability and
  commitment matching.
- P2SH20 and P2SH32 payment destinations. P2SH32 support pays a compiled
  CashScript locking script; it does not execute arbitrary covenant logic.
- Facilitator-side authoritative source-output lookup and raw transaction
  broadcast.

PSBT transport, facilitator sponsorship, `upto`, and legacy batch/debit
settlement remain outside this exact mechanism. Hardware-wallet transport,
WalletConnect, HD discovery, UTXO reservation, and change-address rotation are
wallet integration responsibilities.

## Decisions accepted for the current exact implementation

The following choices are now fixed and must be shared by the
TypeScript and Rust implementations:

- Use fully prefixed CashAddr values at the wire boundary.
- Encode the raw transaction as standard padded RFC 4648 base64.
- Encode BCH amounts as canonical unsigned decimal satoshi strings.
- Require accepted payment data to match the facilitator requirements; unknown
  fields are not an alternate way to change the requested payment.
- Treat `maxTimeoutSeconds` as an off-chain resource/request acceptance window.
  It is not a transaction expiry and cannot be inferred from a BCH transaction
  alone.
- Use standard BCH P2PKH scripts and the ordinary BCH signature hash byte
  `0x41` (`SIGHASH_ALL | SIGHASH_FORKID`) with strict signature/public-key
  validation. The newer `0x61` `UTXOS` flag is not required for this POC.
- Build exactly one merchant output and, when the remainder is above the
  applicable dust threshold, exactly one P2PKH change output. A remainder too
  small to create a standard change output is absorbed into the fee rather than
  emitted as dust.
- Target a fee rate of 1 satoshi per serialized transaction byte. The client
  builds at that rate and the facilitator enforces the minimum; the signed
  transaction's complete byte length is the measurement basis.
- Make settlement acceptance configurable: explicit 0-conf/mempool mode,
  double-spend-proof-assisted mode where the provider supports it, or a
  confirmation-count mode. 0-conf is opt-in and is not the conservative
  default.
- Treat BCH transactions as atomic consensus state transitions. The x402
  error model still needs machine-readable off-chain reasons for invalid
  input, provider, broadcast, conflict, and indeterminate-status conditions;
  these do not represent Ethereum-style partial execution or consumed gas.

The provider/signer "contracts" described below are software API contracts,
not CashScript contracts. The initial implementation does not execute or
validate CashScript: it validates standard P2PKH scripts and BCH transactions.

## Decisions still to lock

These decisions affect interoperability or security and cannot be left to
individual SDK implementations.

### Wire representation

The representation choices are fixed above. The normative scheme document
still needs to spell out the exact maximum amount, equality algorithm, and
unknown-field behavior so that both SDKs implement the same rules rather than
relying on a prose interpretation.

### Transaction policy

- Exact P2PKH input and output script forms, including whether all non-payment
  outputs must also be P2PKH.
- Whether the transaction must have exactly one merchant output and must reject
  additional outputs paying the same script.
- Allowed `nLockTime`, input sequence values, script sizes, transaction size,
  input count, and output count.
- The precise version of the existing BCH relay/dust policy to implement and
  whether a deployment may configure a stricter policy. The POC fee target and
  one-merchant/one-change rule are fixed above, but consensus validity and node
  relay policy remain distinct.
- The allowed locktime, sequence, script-size, transaction-size, input-count,
  and output-count limits.
- How unconfirmed source inputs are treated and which provider states prove an
  outpoint is spendable. The facilitator must fetch the previous output's
  value and locking script itself: transaction inputs contain only outpoints,
  so client-supplied source data cannot be authoritative for BCH sighash or
  value validation.

### Settlement and replay

- The exact configuration and response vocabulary for mempool acceptance,
  cryptographically/verifiably checked BCH double-spend-proof evidence, and a
  required confirmation count. The strategy is configurable, with 0-conf
  opt-in; the default for a public deployment should remain conservative.
- Exact `settlement_pending` response shape and retry/reconciliation rules.
- Whether a repeated identical TXID is idempotent, rejected as already used, or
  accepted only when the resource server supplies an application-level
  idempotency key.
- Protection against reusing one valid broadcast transaction to access several
  paid resources before its inputs are confirmed. This is not solved by BCH's
  UTXO double-spend rule alone; the resource server/facilitator boundary must
  define one-time consumption or request binding.
- The exact stable machine-readable error-code names for malformed
  transactions, invalid signatures, source-output failures, conflicts,
  unsupported tokens, and pending settlement. These are API interoperability
  errors, not claims that a BCH transaction can partially execute.

### SDK and operational surface

- Public package names, registration helpers, and the exact signer/provider
  method surfaces, supported feature flags, and versioning for TypeScript and
  Rust. The conceptual boundary is fixed: the client signer builds/signs; the
  facilitator provider resolves authoritative source outputs, spentness,
  broadcast, and status; neither boundary is a CashScript contract.
- At least one supported BCHN/Fulcrum-compatible provider implementation or a
  documented adapter contract with tested examples.
- Node, Rust, and dependency support ranges; testnet setup; fee configuration;
  logging and privacy expectations; and the compatibility matrix.

## Shared conformance gates before public release

### 1. Normative scheme document

Create one canonical BCH exact specification and use it as the source for both
SDKs. It must define:

- Exact JSON fields, casing, required/optional fields, and payload encoding.
- `bch:bitcoincash` and `bch:bchtest` identity rules.
- Native asset spelling and `assetTransferMethod` behavior.
- `paymentFlow: "upfront"` behavior.
- Satoshi parsing, overflow, and amount matching.
- CashAddr parsing and script equivalence rules.
- P2PKH input validation and P2PKH/P2SH20/P2SH32 payment-output handling.
- Fee, dust, transaction size, input count, output count, and locktime policy.
- Exact merchant-output selection and duplicate-output behavior.
- Payer reporting semantics.
- Mempool versus confirmation success criteria.
- `settlement_pending`, conflict, already-known, and retry behavior.
- Error reason strings and their mapping in both SDKs.

The implementation must fail closed when a requirement is outside this
documented boundary.

### 2. Cross-language golden vectors

The TypeScript and Rust implementations need shared JSON fixtures for:

- Valid one-input and multi-input P2PKH transactions.
- BCH `0x41` signing serialization and signatures.
- Transaction IDs and byte-order conversion.
- CashAddr/script round trips on mainnet and Chipnet.
- Exact output matching with change.
- Wrong network, wrong payee, wrong amount, duplicate payment output, dust,
  excessive fee, malformed transaction, and trailing-byte rejection.
- Missing, spent, conflicting, and substituted source outputs.
- Native BCH and CashToken conservation vectors, including fungible amounts,
  NFT capabilities, NFT commitments, and token change.
- Settlement responses for broadcast success, mempool-only, confirmation,
  conflict, and indeterminate status.

Every successful fixture should decode consistently using libauth and the Rust
implementation. These vectors are more important than matching source-code
structure between SDKs.

### 3. Provider and signer contracts

Separate read-only chain access from signing and broadcast capabilities.

The client side needs UTXO discovery, source transaction data, fee policy, and
a signer capable of producing BCH-valid signatures. The facilitator side needs
source-output lookup, spentness/conflict state, raw transaction broadcast, and
status polling. A facilitator must not trust client-provided source outputs.

The provider contract must distinguish not-found, spent, conflicting, mempool,
confirmed, and transport-error states. A transport error cannot be interpreted
as either spendability or successful settlement.

### 4. Flow integration

The official x402 core supports scheme-specific client/server/facilitator
implementations in a chain mechanism package. BCH should be an external
package such as `@x402/bch` or an OPTN-maintained equivalent, without changing
`@x402/core`. It needs registration helpers, a signer/provider interface,
README examples, and unit/integration tests.

The Rust implementation should remain in `x402-chain-bch`, using the existing
Rust scheme traits, typed v2 aliases, `PriceTag`, and chain-provider patterns.
The current Rust paygate already has a settlement-before-execution mode, but
the BCH integration must make the `upfront` requirement and that configuration
inseparable or fail closed. A wire field alone must not allow an operator to
accidentally run BCH after resource execution.

### 5. Settlement recovery

The raw BCH transaction ID is the natural reconciliation value. After a
successful broadcast with indeterminate confirmation, return
`settlement_pending` with the TXID. A retry must check the existing TXID and
transaction state before attempting any new broadcast. An already-known
identical transaction is not the same as a conflicting transaction.

The first release should document whether `success: true` means accepted into
the mempool or confirmed. A configurable policy is acceptable, but each SDK's
default must be explicit and tested.

## SDK-specific implementation readiness

### Official x402 TypeScript ecosystem

The official architecture is ready for a new mechanism package. The required
surface is the core `SchemeNetworkClient`, `SchemeNetworkServer`, and
`SchemeNetworkFacilitator` interfaces. The closest reference is the official
Aptos exact mechanism because it carries a complete signed transaction and
revalidates it at settlement.

The official repository may accept BCH through its specification-first review
process, but acceptance is not required for interoperability. A separately
published BCH mechanism package can register with `@x402/core` as long as it
uses the canonical wire contract.

### x402-rs

The Rust architecture already has the relevant extension points:

- Chain-specific crate and network identity.
- Generic v2 payload and requirements aliases.
- Client candidate/signer traits.
- Facilitator builder, verification, settlement, and supported-capability
  traits.
- `PriceTag` construction and facilitator-capability enrichment.
- Axum settlement-before-execution support.

The main missing Rust work is BCH-specific: UTXO provider traits, transaction
and sighash primitives, client transaction construction, facilitator policy
checks, raw broadcast/status handling, and a BCH price-tag/server adapter.

The Rust core currently treats settlement responses as generic JSON and does
not provide the official TypeScript payment-flow resolver. BCH can still be
implemented safely by enforcing `upfront` through the BCH integration and
paygate configuration, but this must be covered by integration tests and
documented as an explicit compatibility boundary.

## Recommended implementation order

1. Freeze and publish the scheme document and shared fixtures.
2. Implement libauth-checked BCH primitives and a mock provider in Rust.
3. Implement the Rust client, facilitator, price tag, and Axum flow.
4. Implement the TypeScript BCH mechanism package against `@x402/core`.
5. Run both SDKs against the same fixtures and mocked provider scenarios.
6. Run Chipnet-only end-to-end tests with non-production test funds.
7. Publish integration examples, unsupported-feature limitations, and the
   compatibility matrix.

Only after these gates should the integration be presented as suitable for
general third-party use.
