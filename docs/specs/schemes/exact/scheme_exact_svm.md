# Exact Payment Scheme for Solana Virtual Machine (SVM) (`exact`)

This document specifies the `exact` payment scheme for the x402 protocol on Solana.

This scheme facilitates payments of a specific amount of an SPL token on the Solana blockchain.

## Scheme Name

`exact`

## Protocol Flow

The protocol flow for `exact` on Solana is client-driven.

1.  **Client** makes a request to a **Resource Server**.
2.  **Resource Server** responds with a payment required signal containing `PaymentRequired`. Critically, the `extra` field in the requirements contains a **feePayer** which is the public address of the identity that will pay the fee for the transaction. This is typically the facilitator.
3.  **Client** creates a transaction that contains a transfer of an asset to the resource server's wallet address for a specified amount.
4.  **Client** signs the transaction with their wallet. This results in a partially signed transaction (since the signature of the facilitator that will sponsor the transaction is still missing).
5.  **Client** serializes the partially signed transaction and encodes it as a Base64 string.
6.  **Client** sends a new request to the resource server with the `PaymentPayload` containing the Base64-encoded partially-signed transaction.
7.  **Resource Server** receives the request and forwards the `PaymentPayload` and `PaymentRequirements` to a **Facilitator Server's** `/verify` endpoint.
8.  **Facilitator** decodes and deserializes the proposed transaction.
9.  **Facilitator** inspects the transaction to ensure it is valid and only contains the expected payment instruction.
10. **Facilitator** returns a `VerifyResponse` to the **Resource Server**.
11. **Resource Server**, upon successful verification, forwards the payload to the facilitator's `/settle` endpoint.
12. **Facilitator Server** provides its final signature as the `feePayer` and submits the now fully-signed transaction to the Solana network.
13. Upon successful on-chain settlement, the **Facilitator Server** responds with a `SettlementResponse` to the **Resource Server**.
14. **Resource Server** grants the **Client** access to the resource in its response.

## `PaymentRequirements` for `exact`

In addition to the standard x402 `PaymentRequirements` fields, the `exact` scheme on Solana requires the following inside the `extra` field:

```json
{
  "scheme": "exact",
  "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
  "amount": "1000",
  "asset": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
  "payTo": "2wKupLR9q6wXYppw8Gr2NvWxKBUqm4PPJKkQfoxHDBg4",
  "maxTimeoutSeconds": 60,
  "extra": {
    "feePayer": "EwWqGE4ZFKLofuestmU4LDdK7XM1N4ALgdZccwYugwGd"
  }
}
```

- `asset`: The public key of the token mint.
- `extra.feePayer`: The public key of the account that will pay for the transaction fees. This is typically the facilitator's public key.

## PaymentPayload `payload` Field

The `payload` field of the `PaymentPayload` contains:

```json
{
  "transaction": "AAAAAAAAAAAAA...AAAAAAAAAAAAA="
}
```

The `transaction` field contains the base64-encoded, serialized, **partially-signed** versioned Solana transaction.

Full `PaymentPayload` object:

```json
{
  "x402Version": 2,
  "resource": {
    "url": "https://example.com/weather",
    "description": "Access to protected content",
    "mimeType": "application/json"
  },
  "accepted": {
    "scheme": "exact",
    "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
    "amount": "1000",
    "asset": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
    "payTo": "2wKupLR9q6wXYppw8Gr2NvWxKBUqm4PPJKkQfoxHDBg4",
    "maxTimeoutSeconds": 60,
    "extra": {
      "feePayer": "EwWqGE4ZFKLofuestmU4LDdK7XM1N4ALgdZccwYugwGd"
    }
  },
  "payload": {
    "transaction": "AAAAAAAAAAAAA...AAAAAAAAAAAAA="
  }
}
```

## `SettlementResponse`

The `SettlementResponse` for the exact scheme on Solana:

```json
{
  "success": true,
  "transaction": "base58 encoded transaction signature",
  "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
  "payer": "base58 encoded public address of the transaction fee payer"
}
```

## Facilitator Verification Rules (MUST)

A facilitator verifying an `exact`-scheme SVM payment MUST enforce all of the following checks before sponsoring and signing the transaction:

1. Instruction layout

- The decompiled transaction MUST contain 3 to 5 instructions in this order:
  1. Compute Budget: Set Compute Unit Limit
  2. Compute Budget: Set Compute Unit Price
  3. SPL Token or Token-2022 TransferChecked
  4. (Optional) Lighthouse program instruction (Phantom wallet protection)
  5. (Optional) Lighthouse program instruction (Solflare wallet protection)

- If a 4th or 5th instruction is present, the program MUST be the Lighthouse program (`L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95`).
- Phantom wallet injects 1 Lighthouse instruction; Solflare injects 2.
- These Lighthouse instructions are wallet-injected user protection mechanisms and MUST be allowed to support these wallets.

2. Fee payer (facilitator) safety

- The configured fee payer address MUST NOT appear in the `accounts` of any instruction in the transaction.
- The fee payer MUST NOT be the `authority` for the TransferChecked instruction.
- The fee payer MUST NOT be the `source` of the transferred funds.

3. Compute budget validity

- The program for instructions (1) and (2) MUST be `ComputeBudget` with the correct discriminators (2 = SetLimit, 3 = SetPrice).
- The compute unit price MUST be bounded to prevent gas abuse. The reference implementation enforces ≤ 5 lamports per compute unit.

4. Transfer intent and destination

- The TransferChecked program MUST be either `spl-token` or `token-2022`.
- Destination MUST equal the Associated Token Account PDA for `(owner = payTo, mint = asset)` under the selected token program.

5. Account existence

- The `source` ATA MUST exist.
- The destination ATA MUST exist if and only if the Create ATA instruction is NOT present in the transaction. If Create ATA is present, the destination ATA MAY be absent prior to execution.

6. Amount

- The `amount` in TransferChecked MUST equal `PaymentRequirements.amount` exactly.

These checks are security-critical to ensure the fee payer cannot be tricked into transferring their own funds or sponsoring unintended actions. Implementations MAY introduce stricter limits (e.g., lower compute price caps) but MUST NOT relax the above constraints.
