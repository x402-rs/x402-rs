# x402-chain-aptos

[![Crates.io](https://img.shields.io/crates/v/x402-chain-aptos.svg)](https://crates.io/crates/x402-chain-aptos)
[![Docs.rs](https://docs.rs/x402-chain-aptos/badge.svg)](https://docs.rs/x402-chain-aptos)

Aptos chain support for the x402 payment protocol.

This crate provides implementations of the x402 payment protocol for the Aptos blockchain. It currently supports the V2
protocol with the "exact" payment scheme based on fungible asset transfers with sponsored (gasless) transactions.

## Features

- **V2 Protocol Support**: Implements V2 protocol with CAIP-2 chain ID addressing
- **Fungible Asset Payments**: Token transfers using `0x1::primary_fungible_store::transfer`
- **Sponsored Transactions**: Facilitator pays gas fees for user transactions
- **Transaction Simulation**: Pre-flight validation before settlement
- **Balance Verification**: On-chain balance checks before settlement
- **BCS Encoding**: Binary Canonical Serialization for transaction payloads

## Architecture

The crate is organized into several modules:

- **`chain`** - Core Aptos chain types, providers, and configuration
- **`v2_aptos_exact`** - V2 protocol implementation with CAIP-2 chain IDs

## Feature Flags

- `facilitator` - Facilitator-side payment verification and settlement
- `telemetry` - OpenTelemetry tracing support

## Usage

### Facilitator: Verifying and Settling

```rust
use x402_chain_aptos::{V2AptosExact, AptosChainProvider};
use x402_types::scheme::X402SchemeFacilitatorBuilder;

let provider = AptosChainProvider::from_config( & config).await?;
let facilitator = V2AptosExact.build(provider, None) ?;

// Verify payment
let verify_response = facilitator.verify( & verify_request).await?;

// Settle payment
let settle_response = facilitator.settle( & settle_request).await?;
```

## Supported Networks

The crate includes built-in support for Aptos networks:

- **Aptos Mainnet** (`aptos:1`)
- **Aptos Testnet** (`aptos:2`)

Each network includes USDC token deployment information with proper fungible asset addresses and decimal precision.

## Payment Flow

### Client Side

1. Client receives a `PaymentRequired` response with price tags
2. Client selects a compatible payment option (Aptos + USDC)
3. Client creates a fungible asset transfer transaction
4. Client signs the transaction with their private key
5. Client BCS-encodes and base64-encodes the transaction
6. Client sends the payment payload to the server

### Facilitator Side

1. Facilitator receives the payment payload
2. Deserializes and validates the BCS-encoded transaction
3. Simulates the transaction to verify it will succeed
4. Checks the payer's token balance
5. For verification: Returns success if all checks pass
6. For settlement: Adds sponsor signature and submits the transaction on-chain

## Transaction Structure

Aptos payments use BCS-encoded transactions containing:

- **Entry function payload**: Call to `0x1::primary_fungible_store::transfer`
- **Sender**: The payer's account address
- **Sequence number**: The payer's current sequence number
- **Gas parameters**: Max gas amount and gas unit price
- **Expiration**: Transaction expiration timestamp
- **Chain ID**: The Aptos network identifier

When `sponsor_gas` is enabled, the facilitator adds a fee payer signature before submission.

## Configuration

### Facilitator Configuration Example

```json
{
  "aptos:1": {
    "sponsor_gas": true,
    "signer": "$APTOS_FACILITATOR_KEY",
    "rpc": "https://fullnode.mainnet.aptoslabs.com/v1",
    "api_key": "$APTOS_API_KEY"
  }
}
```

### Configuration Parameters

- **`sponsor_gas`**: Whether to sponsor gas fees (default: false)
- **`signer`**: Hex-encoded Ed25519 private key (required if `sponsor_gas` is true)
- **`rpc`**: Aptos REST API endpoint URL
- **`api_key`**: Optional API key for rate-limited endpoints

## Sponsored Transactions

When `sponsor_gas` is enabled:

1. Client creates and signs a transaction with their account
2. Facilitator validates the transaction
3. Facilitator adds its signature as the fee payer (sponsor)
4. Facilitator submits the dual-signed transaction
5. Facilitator's account pays the gas fees

This allows users to make payments without holding APT for gas fees.

## Dependencies

This crate uses the official Aptos SDK crates:

- `aptos-rest-client` - REST API client for Aptos
- `aptos-types` - Core Aptos types and transaction structures
- `aptos-crypto` - Ed25519 cryptography for signing
- `move-core-types` - Move language types (AccountAddress, etc.)
- `bcs` - Binary Canonical Serialization

## License

Apache 2.0
