# x402-chain-tron

[![Crates.io](https://img.shields.io/crates/v/x402-chain-tron.svg)](https://crates.io/crates/x402-chain-tron)
[![Docs.rs](https://docs.rs/x402-chain-tron/badge.svg)](https://docs.rs/x402-chain-tron)

TRON chain support for the [x402](https://www.x402.org) payment protocol.

This crate provides an implementation of the x402 payment protocol for the TRON blockchain. TRON uses TIP-712 (identical
to EIP-712) for signing, making the authorization struct byte-compatible with EIP-155 at the EIP-712 layer. It currently
supports the V2 protocol with the "exact" payment scheme using EIP-3009-style `transferWithAuthorization` and Permit2.

## Features

- **V2 Protocol Support**: Implements V2 protocol with CAIP-2 chain ID addressing (`tron:` namespace)
- **EIP-3009 Payments**: Gasless token transfers using `transferWithAuthorization` via TIP-712 typed-data signing
- **Permit2 Payments**: Universal gasless token transfers using SUN.io's Permit2 contract
- **EOA-only Signatures**: TRON has no contract wallets — only secp256k1 ecrecover (`eth_sign`-style)
- **Base58Check Addresses**: Native TRON address encoding on the wire; EVM hex in EIP-712 payloads
- **TronGrid HTTP API**: Settlement via TronGrid REST API (not alloy/JSON-RPC providers)
- **Transaction Polling**: Configurable timeout and poll interval for confirmation

## Architecture

The crate is organized into several modules:

- **`chain`** - Core TRON chain types, provider, address handling, and configuration
- **`v2_tron_exact`** - V2 protocol implementation with CAIP-2 chain IDs (`tron:…`)

## Feature Flags

- `facilitator` - Facilitator-side payment verification and settlement
- `telemetry` - OpenTelemetry tracing support

## Usage

### Facilitator: Verifying and Settling

```rust
use x402_chain_tron::{V2TronExact, TronChainProvider};
use x402_types::scheme::X402SchemeFacilitatorBuilder;

let provider = TronChainProvider::from_config(&config).await?;
let facilitator = V2TronExact.build(provider, None)?;

// Verify payment
let verify_response = facilitator.verify(&verify_request).await?;

// Settle payment
let settle_response = facilitator.settle(&settle_request).await?;
```

## Supported Networks

The crate includes built-in support for TRON networks via the `KnownNetworkTron` trait:

| Network        | CAIP-2              | Chain ID   |
|----------------|---------------------|------------|
| TRON Mainnet   | `tron:0x2b6653dc`   | 728126428  |
| TRON Nile      | `tron:0xcd8690dc`   | 3448148188 |

Each network includes USDT token deployment information with proper contract addresses, decimal precision, and transfer
method.

### Contract Addresses

**Mainnet**
- **SUN.io Permit2**: `TTJxU3P8rHycAyFY4kVtGNfmnMH4ezcuM9`
- **X402ExactPermit2Proxy**: `TNtw4Wg6uQe4bqFywtcn5qagVZesSdYBSs`
- **USDT**: `TR7NHqjeKQxGTCi8q8ZY4pL8otSzgjLj6t`

**Nile Testnet**
- **SUN.io Permit2**: `TCJjTtzwRJYPapGTdyJdKcr7MqkngRRWQx`
- **X402ExactPermit2Proxy**: `TTjbkCh8sC4gNTWG48iWNssrLBqZq2tiTy`
- **USDT**: `TXLAQ63Xg1NAzckPwKHvzw7CSEmLMEqcdj`

## Payment Flow

### Client Side

1. Client receives a `PaymentRequired` response with price tags
2. Client selects a compatible payment option (TRON + USDT)
3. Client creates a TIP-712 typed-data authorization (EIP-3009 or Permit2)
4. Client signs the authorization with their secp256k1 private key
5. Client sends the payment payload to the server

### Facilitator Side

1. Facilitator receives the payment payload
2. Deserializes and validates the typed-data authorization
3. Recovers the signer address via secp256k1 ecrecover
4. Checks the payer's token balance via TronGrid
5. For verification: Returns success if all checks pass
6. For settlement: Broadcasts the transaction via TronGrid HTTP API and polls for confirmation

## Asset Transfer Methods

Payment requirements specify an `assetTransferMethod`:

- **`eip3009`**: Direct `transferWithAuthorization` for tokens with native EIP-3009 support (e.g. USDT on TRON)
- **`permit2`**: Universal proxy using SUN.io's canonical Permit2 contract

## TRON vs EVM Differences

| Aspect | TRON | EVM (EIP-155) |
|---|---|---|
| Address format (wire) | Base58Check (T…) | Hex (0x…) |
| Address in EIP-712 | EVM hex (0x…) | EVM hex (0x…) |
| Contract wallets | ❌ Not supported | ✅ EIP-1271 / EIP-6492 |
| RPC protocol | TronGrid HTTP REST | JSON-RPC |
| Chain namespace | `tron:` | `eip155:` |
| Chain reference | Last 4 bytes of genesis hash (`0x…`) | Decimal chain ID |

## Configuration

### Facilitator Configuration Example

```json
{
  "tron:0x2b6653dc": {
    "rpc_url": "https://api.trongrid.io",
    "signers": ["$TRON_FACILITATOR_KEY"],
    "tx_timeout_secs": 60,
    "tx_poll_interval_secs": 3
  }
}
```

### Configuration Parameters

- **`rpc_url`**: TronGrid HTTP API base URL (literal URL or `$ENV_VAR` reference)
- **`signers`**: One or more hex-encoded secp256k1 private keys (with or without `0x` prefix), or `$ENV_VAR` references
- **`contracts`** *(optional)*: Override well-known contract addresses for `sun_permit2` and `x402_exact_permit2_proxy`
- **`tx_timeout_secs`**: How long to wait for transaction confirmation before giving up (default: 60)
- **`tx_poll_interval_secs`**: How often to poll `gettransactioninfobyid` (default: 3)

## Dependencies

This crate uses:

- `reqwest` - HTTP client for TronGrid REST API calls
- `k256` - secp256k1 ECDSA signing and recovery
- `alloy-primitives` / `alloy-sol-types` - ABI encoding for contract calls
- `bs58` / `sha2` - Base58Check address encoding and decoding

## License

Apache 2.0
