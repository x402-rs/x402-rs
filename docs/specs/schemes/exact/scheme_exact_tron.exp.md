---
Document Type: Scheme
Description: Experimental - Exact payment scheme specification for TRON network (pending upstream merge)
Source PR: https://github.com/x402-foundation/x402/pull/2076
Additional PR: https://github.com/x402-foundation/x402/pull/1408
Downloaded At: 2026-06-16
Experimental: true
Note: DELETE this file once either PR lands in upstream x402-foundation/x402 and the canonical spec is pulled via the normal update process.
---
# Scheme: `exact` on TRON

## Summary

The `exact` scheme on TRON executes a transfer where the Facilitator (server) pays the TRON energy/bandwidth, but the Client (user) controls the exact flow of funds via TIP-712 signatures. TIP-712 ([tip-712.md](https://github.com/tronprotocol/tips/blob/master/tip-712.md), status `Final`) is TRON's implementation of EIP-712 and uses the identical signing format and domain separator structure.

**This spec mirrors [`scheme_exact_evm.md`](./scheme_exact_evm.md) byte-for-byte at the payload-schema and EIP-712 layers.** Same JSON field names (`permit2Authorization`, `authorization`, `witness`, etc.), same typehashes, same `TokenPermissions` / `PermitTransferFrom` / `Witness` struct layouts. Only two things change on TRON: the address format (Base58 in `paymentRequirements`, `0x` hex inside the signed payload) and the on-chain SDK (TronWeb instead of viem/ethers).

Two asset-transfer methods are supported, selected by `paymentRequirements.extra.assetTransferMethod`:

| `assetTransferMethod` | Use case | Recommendation | Usage semantics |
|---|---|---|---|
| **1. `eip3009`** | Tokens with native `transferWithAuthorization` (e.g. ERC-3009-compatible TRC-20) | **Recommended** (simplest, truly gasless) | One-time use |
| **2. `permit2`** | Tokens without EIP-3009. Uses SUN.io Permit2 + `x402ExactPermit2Proxy` | **Universal fallback** (works for any TRC-20) | One-time use |

`ERC-7710` (delegation) is not supported on TRON — TRON has no equivalent smart-account delegation framework.

If no `assetTransferMethod` is specified in the payload, implementations should prioritize `eip3009` (if the token supports it) and then `permit2`.

TRON does **not** support ERC-1271, ERC-6492, or Multicall3. Signature verification is `ecrecover`-only. Facilitator diagnostics use sequential `triggerConstantContract` calls in place of Multicall3.

TRON does **not** have a formal TIP-3009 standard in `tronprotocol/tips` today. This spec references EIP-3009 directly for the on-chain `transferWithAuthorization` interface, following the USDC precedent of shipping the interface ahead of a formal standard document. TIP-712 (the signing layer) is `Final`.

## Address Conventions

- `paymentRequirements` fields (`asset`, `payTo`) use TRON Base58 (`T...`).
- `payload.authorization` / `payload.permit2Authorization` addresses use EVM hex (`0x...`) — identical to EVM spec.
- Conversion: Base58Check → drop the prefix byte (`0x41`) → `0x` + the remaining 20 bytes hex.

## 1. AssetTransferMethod: `eip3009`

The `eip3009` method uses the `transferWithAuthorization` function directly on TRC-20 contracts that implement the ERC-3009 interface.

### Phase 1: `PAYMENT-SIGNATURE` Header Payload

Identical structure to [`scheme_exact_evm.md`](./scheme_exact_evm.md) `eip3009`. The `payload` field contains:

- `signature`: The 65-byte signature of the `transferWithAuthorization` operation.
- `authorization`: Parameters required to reconstruct the signed message.

**Example PaymentPayload:**

```json
{
  "x402Version": 2,
  "accepted": {
    "scheme": "exact",
    "network": "tron:nile",
    "amount": "100000",
    "asset": "T...",
    "payTo": "TYukBQZ2XXCehNLMRhRx6A4XKXD7cT6bnX",
    "maxTimeoutSeconds": 3600,
    "extra": {
      "assetTransferMethod": "eip3009",
      "name": "x402 Test USD",
      "version": "1"
    }
  },
  "payload": {
    "signature": "0x...",
    "authorization": {
      "from": "0x...",
      "to": "0x...",
      "value": "100000",
      "validAfter": "1713000000",
      "validBefore": "1713003600",
      "nonce": "0x..."
    }
  }
}
```

Signature is a TIP-712 signature over the `TransferWithAuthorization` struct with domain `{ name, version, chainId: <block.chainid>, verifyingContract: <token address, hex> }`.

### Phase 2: Verification Logic

Identical to EVM spec, adapted for TRON:

1. **Verify** the signature is valid and recovers to `authorization.from` (ecrecover only; no ERC-1271).
2. **Verify** the `client` has sufficient balance of the `asset`.
3. **Verify** the authorization parameters (amount, validity window) meet the `PaymentRequirements`.
4. **Verify** the token and network match the requirement; `authorization.to` equals `payTo` (after Base58→hex conversion).
5. **Simulate** `token.transferWithAuthorization(...)` via `triggerConstantContract` to ensure success.
6. On simulation failure, diagnose sequentially (no Multicall3): `balanceOf(from) ≥ value`, `authorizationState(from, nonce) == false`, token exposes the ERC-3009 ABI.

### Phase 3: Settlement Logic

Settlement is performed by the facilitator calling `transferWithAuthorization` on the ERC-3009-compatible TRC-20 via `triggerSmartContract` with the `payload.signature` and `payload.authorization` parameters.

- selector: `transferWithAuthorization(address,address,uint256,uint256,uint256,bytes32,uint8,bytes32,bytes32)`
- `feeLimit`: 100 TRX (configurable)
- Wait for confirmation via `getTransactionInfo(txid)` (~3s per block)

---

## 2. AssetTransferMethod: `permit2`

This method mirrors EVM `exact.permit2` exactly. It uses `permitWitnessTransferFrom` on SUN.io's Permit2 deployment combined with a TRON-deployed `x402ExactPermit2Proxy` (see [Annex](#reference-implementation-x402exactpermit2proxy-on-tron)) to enforce receiver-address security via the Witness pattern. The Witness pattern locks `to` so the facilitator cannot alter the destination.

### Phase 1: One-Time Gas Approval

Permit2 requires the user to approve the Permit2 contract to spend their tokens. This is a one-time setup. **TRON differs from EVM here in one material way: TRC-20's `approve()` requires `msg.sender` to be the token owner, so the Facilitator cannot sponsor `approve()` on the user's behalf.** Two options are available on TRON (EVM's "Option B: Sponsored ERC-20 Approval" is not supported):

#### TRON Option 1: Direct User Approval (Standard)

The user submits a standard on-chain `approve(Permit2)` transaction paying their own energy/bandwidth.

- *Prerequisite:* User must have TRX or staked resources.

#### TRON Option 2: EIP-2612 `permit` (Extension: [`eip2612GasSponsoring`](../../extensions/eip2612_gas_sponsoring.md))

If the TRC-20 supports EIP-2612, the user signs a `permit` authorizing Permit2 and the facilitator submits it.

- *Prerequisite:* Token supports EIP-2612.
- *Flow:* Facilitator calls `x402ExactPermit2Proxy.settleWithPermit()`.

Implementations MUST return `412 Precondition Failed` (error `PERMIT2_ALLOWANCE_REQUIRED`) if neither option applies and the user has no prior Permit2 allowance.

### Phase 2: `PAYMENT-SIGNATURE` Header Payload

Identical structure to [`scheme_exact_evm.md`](./scheme_exact_evm.md) `permit2`. The `payload` field contains:

- `signature`: The signature for `permitWitnessTransferFrom`.
- `permit2Authorization`: Parameters to reconstruct the message.

**Important Logic (identical to EVM):** The `spender` in the signature is the TRON-deployed [`x402ExactPermit2Proxy`](#reference-implementation-x402exactpermit2proxy-on-tron) — NOT the facilitator. The Proxy enforces that funds go to `witness.to`.

**Example PaymentPayload:**

```json
{
  "x402Version": 2,
  "accepted": {
    "scheme": "exact",
    "network": "tron:nile",
    "amount": "100000",
    "asset": "TXLAQ63Xg1NAzckPwKHvzw7CSEmLMEqcdj",
    "payTo": "TYukBQZ2XXCehNLMRhRx6A4XKXD7cT6bnX",
    "maxTimeoutSeconds": 3600,
    "extra": {
      "assetTransferMethod": "permit2",
      "name": "USDT",
      "version": "1"
    }
  },
  "payload": {
    "signature": "0x...",
    "permit2Authorization": {
      "permitted": {
        "token": "0x...",
        "amount": "100000"
      },
      "from": "0x...",
      "spender": "0x...",
      "nonce": "<bitmap nonce>",
      "deadline": "1713003600",
      "witness": {
        "to": "0x...",
        "validAfter": "1713000000"
      }
    }
  }
}
```

Signature is a TIP-712 signature over `PermitTransferFrom` with the Witness, using the SUN.io Permit2 contract as the EIP-712 domain `verifyingContract` (`{ name: "Permit2", chainId: <block.chainid>, verifyingContract: <Permit2 address, hex> }`). The `WITNESS_TYPE_STRING` and `WITNESS_TYPEHASH` are byte-identical to the EVM reference implementation.

### Phase 3: Verification Logic

Identical ordering to EVM spec:

1. **Verify** `payload.signature` is valid and recovers to `permit2Authorization.from` (ecrecover only).
2. **Verify** the `client` has enabled the Permit2 approval:
   - If `TRC20.allowance(from, Permit2) < amount`:
     - Check for **EIP-2612 Permit** (Extension): refer to [`eip2612GasSponsoring`](../../extensions/eip2612_gas_sponsoring.md).
     - **Sponsored ERC-20 Approval is NOT available on TRON** (see §2 Phase 1 above — `approve()` requires `msg.sender` to be the token owner).
     - **If neither applies:** return `412 Precondition Failed` (`PERMIT2_ALLOWANCE_REQUIRED`). The client must submit a one-time direct `approve(Permit2)` before retrying.
3. **Verify** the `client` has sufficient balance of the `asset`.
4. **Verify** `permit2Authorization.permitted.amount` covers the payment.
5. **Verify** the `deadline` (not expired) and `witness.validAfter` (active). TRON block-time buffer: `deadline > now + 6s`.
6. **Verify** the token and network match the requirement; `witness.to` equals `payTo` (after Base58→hex conversion).
7. **Pre-flight allowance check (recommended):** facilitator MAY call `Permit2Helper.checkPermit2Allowance(permit2, token, owner, spender, amount)` if the Helper is deployed on the network, or call `Permit2.allowance(owner, token, spender)` directly.
8. **Simulation (Recommended):**
   - *Standard:* simulate `x402ExactPermit2Proxy.settle` via `triggerConstantContract`.
   - *With EIP-2612 Permit (Extension):* simulate `x402ExactPermit2Proxy.settleWithPermit`.

### Phase 4: Settlement Logic

Settlement is performed by calling the TRON-deployed `x402ExactPermit2Proxy` via `triggerSmartContract`:

1. **Standard Settlement:** `x402ExactPermit2Proxy.settle(permit, owner, witness, signature)` — if the user has sufficient Permit2 allowance.
2. **With EIP-2612 Permit (Extension):** `x402ExactPermit2Proxy.settleWithPermit(permit2612, permit, owner, witness, signature)` — batched.

- `feeLimit`: 100 TRX (configurable)
- Wait for confirmation via `getTransactionInfo(txid)`

The facilitator is the `owner_address` of the on-chain tx, so the facilitator's TRON account pays the energy/bandwidth. The user never needs to hold TRX.

---

## Implementer Notes

- **Permit2 Dependency:** SUN.io's Permit2 is a byte-identical fork of Uniswap Permit2 (see [Annex: SUN.io Permit2 Deployments](#annex-sunio-permit2-deployments)). `x402ExactPermit2Proxy` is a straightforward port of the EVM reference. Integrators inherit the security properties of both contracts.
- **No CREATE2 across networks:** Unlike EVM where `x402ExactPermit2Proxy` is CREATE2-deployed to the same address on all chains, TRON's `x402ExactPermit2Proxy` address is network-specific and listed in the Annex.

---

## Annex

### SUN.io Permit2 Deployments

| Network | Permit2 | Permit2Helper (optional) |
|---|---|---|
| Mainnet | `TTJxU3P8rHycAyFY4kVtGNfmnMH4ezcuM9` (TronScan-verified, 29,000+ live txs) | `TBc4z7389sAtM2nZRgWwHSJnHrWeUrZ3rL` |
| Nile | `TCJjTtzwRJYPapGTdyJdKcr7MqkngRRWQx` | `TJcVB8vQVpAoGwp9owx1Ct91D4QpKVd78h` |

Source code: https://github.com/sun-protocol/sunswap-permit2

Interface compatibility with Uniswap Permit2:

- EIP-712 domain typehash, nameHash, struct layouts, function signatures: byte-identical.
- `DOMAIN_SEPARATOR` uses `block.chainid` at full value (no truncation). See `contracts/EIP712.sol`.
- Nonce scheme: bitmap, 248-bit wordPos + 8-bit bitPos (same as Uniswap).

`Permit2Helper` is a SUN.io convenience contract with no EVM analogue. It returns `false` if any of:
- `lastAmount > currentAllowanceAmount`
- `block.timestamp > expiration`
- `expiration - block.timestamp ≤ 200` **seconds**

The Helper is optional — facilitators can call `Permit2.allowance()` directly for full symmetry with EVM.

### Reference Implementation: `x402ExactPermit2Proxy` on TRON

A TRON port of the EVM reference `x402ExactPermit2Proxy` (see [`scheme_exact_evm.md`](./scheme_exact_evm.md#reference-implementation-x402exactpermit2proxy)). Same Solidity source, deployed via TRON's Solidity toolchain (`solc` compatible via `TVM`); same `WITNESS_TYPEHASH`, `WITNESS_TYPE_STRING`, `settle`, and `settleWithPermit` functions.

**Deployments:**

| Network | `x402ExactPermit2Proxy` |
|---|---|
| Mainnet | TBD |
| Nile | TBD |

BofAI/SUN.io will deploy and TronScan-verify this contract on Nile (and optionally Mainnet) before PR1 merges; addresses will be filled via a follow-up commit to this branch.

### Supported Tokens

**`permit2` path.** Every TRC-20 is eligible, provided the user has a Permit2 allowance on that token (or signs EIP-2612 `permit`, or sends a manual `approve`). Notable:

| Token | Mainnet | Nile |
|---|---|---|
| USDT | `TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t` | `TXLAQ63Xg1NAzckPwKHvzw7CSEmLMEqcdj` |
| USDD | `TPYmHEhy5n8TCEfYGqW2rPxsghSfzghPDn` | — |

**`eip3009` path.** Only tokens that implement the ERC-3009 interface qualify.

| Token | Mainnet | Nile |
|---|---|---|
| x402 Test USD (BofAI reference) | TBD | TBD |

BofAI will deploy and TronScan-verify the reference ERC-3009-compatible TRC-20 on Nile before PR1 merges; addresses will be filled via a follow-up commit to this branch.

### Chain IDs

| Network | CAIP-2 | Chain ID |
|---|---|---|
| Mainnet | `tron:mainnet` | `728126428` |
| Nile | `tron:nile` | `3448148188` |

Shasta is not included — it lags Nile on features and does not allow external nodes.

### Error Codes

Error codes mirror EVM equivalents where possible. TRON-specific:

**Shared**
- `invalid_exact_tron_scheme`
- `invalid_exact_tron_network_mismatch`
- `invalid_exact_tron_missing_eip712_domain`
- `invalid_exact_tron_recipient_mismatch`
- `invalid_exact_tron_signature`
- `invalid_exact_tron_authorization_value`
- `invalid_exact_tron_insufficient_balance`
- `invalid_exact_tron_transaction_simulation_failed`
- `invalid_exact_tron_transaction_failed`

**`eip3009`-specific**
- `invalid_exact_tron_payload_authorization_valid_before`
- `invalid_exact_tron_payload_authorization_valid_after`
- `invalid_exact_tron_nonce_already_used`
- `invalid_exact_tron_eip3009_not_supported`

**`permit2`-specific**
- `PERMIT2_ALLOWANCE_REQUIRED` (HTTP 412; same code as EVM)
- `invalid_exact_tron_deadline_expired`
- `invalid_exact_tron_permit2_allowance_insufficient`
- `invalid_exact_tron_permit2_allowance_expired`
- `invalid_exact_tron_permit2_proxy_not_deployed`
