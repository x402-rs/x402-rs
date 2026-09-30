# BCH x402 open decisions

This file is retained as a design record. The implementation decisions below
were made for the current exact mechanism; unresolved future work is called
out explicitly rather than presented as an unimplemented current feature.

## POC versus long-term interoperability

The current exact mechanism uses a finalized signed transaction as the payment
payload. It supports native BCH, CashTokens, and P2PKH/P2SH20/P2SH32 payment
outputs. These are transaction-level capabilities; they do not make x402 a
general CashScript execution or covenant framework.

The implementation should keep a script-, UTXO-, and asset-first internal
model so future work can add additional address encodings and PSBT
signing/interchange without changing the meaning of the finalized transaction
settlement object.

## 1. Native asset spelling

Recommended default: use:

```json
{
  "asset": "BCH",
  "extra": {
    "assetTransferMethod": "native"
  }
}
```

Rationale: the network already identifies BCH, while `BCH` is clear to
developers and matches the native-symbol style used by other x402 scheme
specifications. The implementation should accept exactly one spelling.

## 2. Settlement timing

Recommended default: advertise the canonical x402 v2 payment flow as
`extra.paymentFlow: "upfront"` for native BCH exact. In `x402-rs`, this maps
to the paygate's `settle_before_execution = true` configuration.

The resulting ordering is:

```text
settle (re-validate and broadcast) -> resource -> response
```

The facilitator's `/verify` endpoint remains useful as a read-only diagnostic,
but it must not be treated as the authorization gate for the resource. The
upfront flow establishes payment by settlement itself.

Rationale: `/verify` cannot reserve a UTXO. A payer can double-spend an input
between verification and settlement. Broadcasting before resource execution
provides a stronger guarantee than a verify-only authorization flow and uses
the flow vocabulary defined by canonical x402.

## 3. Confirmation threshold

Recommended default: make the settlement strategy configurable. Supported
initial strategies should include explicit `mempool accepted` (0-conf), a
provider capability for verified BCH double-spend-proof evidence, and a
required confirmation count. 0-conf must be an explicit opt-in; at least one
confirmation remains the conservative production mode.

The settlement response must distinguish confirmed, mempool-only,
double-spend-proof-assisted, and pending states. A transport error after
broadcast must not cause an unsafe blind retry. A double-spend proof is
additional network-state evidence for an unconfirmed transaction, not a reason
to skip transaction, source-output, or conflict validation.

## 4. Initial script scope

Recommended default: standard P2PKH only, with multiple inputs allowed.

This permits independent BCH signature verification without pretending to
support arbitrary CashScript or covenant semantics. P2SH, P2SH32, and arbitrary
CashScript should be separate milestones. Additional address encodings may be
accepted in a later phase, but they must resolve to a validated locking script
before transaction policy checks are performed.

The POC transaction shape is one merchant output plus one P2PKH change output
when the remainder meets the applicable dust threshold. A below-dust remainder
is added to the fee rather than emitted as an invalid dust output. The target
fee rate is 1 satoshi per serialized byte, measured on the complete signed
transaction.

## 5. Source-output transport

Recommended default: the client sends only the signed transaction; the
facilitator fetches source outputs from its configured provider.

Client-supplied source outputs may be useful as a cache hint, but must never be
authoritative. If they are added later, the facilitator must compare them with
chain data.

This is a software/provider boundary, not a CashScript contract. The
facilitator needs the authoritative previous-output value and locking script to
validate BCH sighash preimages, P2PKH signatures, and value conservation;
those fields are not carried in a transaction input itself. Unknown provider
state must fail closed.

## 6. Local test identity

Recommended default: keep public identities limited to `bch:bitcoincash` and
`bch:bchtest`, and configure local tests through an explicit test-provider mode.

If a public wire identity for regtest is needed, add it only after verifying the
BCH ecosystem convention and record it as a new decision. Do not invent
`bch:mainnet` or `bip122:*` aliases for convenience.

## 7. CashTokens

Implemented boundary: token-bearing inputs and outputs are accepted only when
the requested category, fungible amount, NFT capability, and NFT commitment
are conserved and the merchant output matches the request. BCH value and token
state are validated independently.

Parsing a CashToken prefix is not equivalent to proving token correctness.
Genesis/provenance rules, arbitrary successor covenants, and minting workflows
remain outside x402 settlement. A token-aware address must not silently turn a
token payment into a native-BCH payment or vice versa.

## 8. Payer reporting

Recommended default: report a deterministic address derived from the first
valid P2PKH input as an informational payer value. Security checks must validate
all inputs independently and must not rely on the reported payer string.

## 9. Fee sponsorship

Recommended default: client pays its own fee. Do not allow facilitators to
append inputs or mutate a signed transaction. Sponsorship requires a separately
specified sighash and signing protocol.

## 10. Address format extensibility

Recommended default: treat addresses as input/output encodings at the API
boundary, then normalize them to an internal network-qualified locking script
plus any explicit token metadata.

The POC should use CashAddr, but future support may include legacy Base58
addresses, token-aware CashToken addresses, and other BCH ecosystem formats.
The parser must validate the network, address type, checksum, payload length,
and token metadata before producing a script. Network or asset identity must
never be inferred from a text prefix alone, and serializing an address must not
discard token information.

## 11. PSBT support

Recommended default: keep the initial x402 payment payload as a finalized raw
transaction. Add PSBT as a wallet and signing interchange format, not as an
alternative settlement object, until BCH-specific PSBT semantics are defined.

Future PSBT support should cover unsigned transaction data, authoritative
source outputs, BCH sighash metadata, partial signatures, hardware-wallet
signing, finalization, and preservation of unknown/proprietary fields. The
facilitator should validate the finalized transaction using the same rules
regardless of whether the client assembled it directly or finalized it from a
PSBT. A PSBT should not be accepted for settlement merely because it can be
parsed; it must produce the exact signed transaction being verified and
broadcast.
