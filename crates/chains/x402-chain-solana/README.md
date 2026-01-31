# x402-chain-solana

[![Crates.io](https://img.shields.io/crates/v/x402-chain-solana.svg)](https://crates.io/crates/x402-chain-solana)
[![Docs.rs](https://docs.rs/x402-chain-solana/badge.svg)](https://docs.rs/x402-chain-solana)

Solana chain support for the x402 payment protocol.

This crate provides implementations of the x402 payment protocol for the Solana blockchain. It supports both V1 and V2 protocol versions with the "exact" payment scheme based on SPL Token `transfer` instructions with pre-signed authorization.

## Features

- **V1 and V2 Protocol Support**: Implements both protocol versions with network name (V1) and CAIP-2 chain ID (V2) addressing
- **SPL Token Payments**: Token transfers using pre-signed transaction authorization
- **Compute Budget Management**: Automatic compute unit limit and price configuration for transaction prioritization
- **WebSocket Support**: Optional pubsub for faster transaction confirmation via signature subscriptions
- **Balance Verification**: On-chain balance checks before settlement
- **Transaction Simulation**: Pre-flight simulation to validate transactions before submission

## Architecture

The crate is organized into several modules:

- **`chain`** - Core Solana chain types, providers, and configuration
- **`v1_solana_exact`** - V1 protocol implementation with network names
- **`v2_solana_exact`** - V2 protocol implementation with CAIP-2 chain IDs

## Feature Flags

- `server` - Server-side price tag generation
- `client` - Client-side payment signing
- `facilitator` - Facilitator-side payment verification and settlement
- `telemetry` - OpenTelemetry tracing support

## Usage

### Server: Creating a Price Tag

```rust
use x402_chain_solana::{V1SolanaExact, KnownNetworkSolana};
use x402_types::networks::USDC;

// Get USDC deployment on Solana mainnet
let usdc = USDC::solana();

// Create a price tag for 1 USDC
let price_tag = V1SolanaExact::price_tag(
    "9WzDXwBbmkg8ZTbNMqUxvQRAyrZzDsGYdLVL9zYtAWWM",
    usdc.amount(1_000_000u64),
);
```

### Client: Signing a Payment

```rust
use x402_chain_solana::V1SolanaExactClient;
use solana_keypair::Keypair;

let keypair = Keypair::new();
let client = V1SolanaExactClient::new(keypair);

// Use client to sign payment candidates
let candidates = client.accept(&payment_required);
```

### Facilitator: Verifying and Settling

```rust
use x402_chain_solana::{V1SolanaExact, SolanaChainProvider};
use x402_types::scheme::X402SchemeFacilitatorBuilder;

let provider = SolanaChainProvider::from_config(&config).await?;
let facilitator = V1SolanaExact.build(provider, None)?;

// Verify payment
let verify_response = facilitator.verify(&verify_request).await?;

// Settle payment
let settle_response = facilitator.settle(&settle_request).await?;
```

## Supported Networks

The crate includes built-in support for Solana networks through the `KnownNetworkSolana` trait:

- **Solana Mainnet** (`solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp`)
- **Solana Devnet** (`solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1`)

Each network includes USDC token deployment information with proper mint addresses and decimal precision.

## Payment Flow

### Client Side

1. Client receives a `PaymentRequired` response with price tags
2. Client selects a compatible payment option (Solana + USDC)
3. Client creates a pre-signed SPL Token transfer transaction
4. Client serializes and base64-encodes the transaction
5. Client sends the payment payload to the server

### Facilitator Side

1. Facilitator receives the payment payload
2. Deserializes and validates the transaction structure
3. Simulates the transaction to verify it will succeed
4. Checks the payer's token balance
5. For verification: Returns success if all checks pass
6. For settlement: Submits the transaction on-chain and waits for confirmation

## Transaction Structure

Solana payments use a pre-signed `VersionedTransaction` containing:

- **Transfer instruction**: SPL Token transfer from payer to recipient
- **Compute budget instructions**: Set compute unit limit and price
- **Signatures**: Pre-signed by the payer, co-signed by the facilitator (fee payer)

The facilitator adds its signature as the fee payer and submits the transaction.

## Configuration

### Facilitator Configuration Example

```json
{
  "solana": {
    "5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp": {
      "signer": "$SOLANA_FACILITATOR_KEY",
      "rpc": "https://api.mainnet-beta.solana.com",
      "pubsub": "wss://api.mainnet-beta.solana.com",
      "max_compute_unit_limit": 400000,
      "max_compute_unit_price": 1000000
    }
  }
}
```

### Configuration Parameters

- **`signer`**: Base58-encoded 64-byte Solana keypair (or environment variable reference)
- **`rpc`**: HTTP RPC endpoint URL
- **`pubsub`**: Optional WebSocket endpoint for faster confirmations
- **`max_compute_unit_limit`**: Maximum compute units per transaction (default: 400,000)
- **`max_compute_unit_price`**: Maximum price per compute unit in micro-lamports (default: 1,000,000)

## Compute Budget

Solana transactions have compute unit limits that determine how much computation they can perform. The facilitator automatically adds compute budget instructions to transactions:

- **Compute Unit Limit**: Maximum compute units the transaction can consume
- **Compute Unit Price**: Priority fee in micro-lamports per compute unit

Higher compute unit prices increase transaction priority during network congestion.

## Dependencies

This crate uses the official Solana SDK crates:

- `solana-client` - RPC and WebSocket client
- `solana-transaction` - Transaction building and signing
- `solana-keypair` - Ed25519 keypair management
- `spl-token` - SPL Token program interactions

## License

Apache 2.0
