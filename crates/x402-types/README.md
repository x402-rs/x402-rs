# x402-types

Core types for the x402 payment protocol.

This crate provides the foundational types used throughout the x402 ecosystem for implementing HTTP 402 Payment Required flows. It is designed to be blockchain-agnostic, with chain-specific implementations provided by separate crates.

## Overview

The x402 protocol enables micropayments over HTTP by leveraging the 402 Payment Required status code. When a client requests a paid resource, the server responds with payment requirements. The client signs a payment authorization, which is verified and settled by a facilitator.

```text
┌────────┐         ┌────────┐         ┌─────────────┐
│ Client │ ──1──▶  │ Server │ ──2──▶  │ Facilitator │
│        │ ◀──4──  │        │ ◀──3──  │             │
└────────┘         └────────┘         └─────────────┘

1. Request paid resource
2. Verify payment with facilitator
3. Payment valid / settled
4. Return resource (or 402 Payment Required)
```

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
x402-types = "0.1"
```

With telemetry support:

```toml
[dependencies]
x402-types = { version = "0.1", features = ["telemetry"] }
```

## Modules

| Module        | Description                                                                        |
|---------------|------------------------------------------------------------------------------------|
| `chain`       | Blockchain identifiers and provider abstractions (CAIP-2 chain IDs)                |
| `config`      | Server configuration, CLI parsing, RPC config, and environment variable resolution |
| `facilitator` | Core trait for payment verification and settlement                                 |
| `networks`    | Registry of well-known blockchain networks                                         |
| `proto`       | Wire format types for protocol messages (V1 and V2)                                |
| `scheme`      | Payment scheme system for extensible payment methods                               |
| `timestamp`   | Unix timestamp utilities for payment authorization windows                         |
| `util`        | Helper types (base64, string literals, money amounts)                              |

## Protocol Versions

The crate supports two protocol versions:

- **V1** (`proto::v1`): x402 protocol v1
- **V2** (`proto::v2`): x402 protocol v2

## Key Types

### Chain Identifiers

```rust
use x402_types::chain::ChainId;

// Parse a CAIP-2 chain ID
let chain_id: ChainId = "eip155:8453".parse().unwrap();
assert_eq!(chain_id.namespace(), "eip155");
assert_eq!(chain_id.reference(), "8453");

// Convert from network name (V1 compatibility)
let chain_id = ChainId::from_network_name("base").unwrap();
```

### Payment Requirements (V2)

```rust
use x402_types::proto::v2::{PaymentRequirements, PriceTag};

// Payment requirements returned in 402 response
let requirements = PaymentRequirements {
    scheme: "exact".to_string(),
    chain_id: "eip155:8453".parse().unwrap(),
    pay_to: "0x...".to_string(),
    max_amount_required: "1000000".to_string(), // in smallest units
    resource: "https://api.example.com/premium".to_string(),
    // ... other fields
};
```

### Facilitator Trait

```rust
use x402_types::facilitator::Facilitator;
use x402_types::proto::v2::{VerifyRequest, VerifyResponse, SettleRequest, SettleResponse};

// Implement for your payment verification service
#[async_trait::async_trait]
impl Facilitator for MyFacilitator {
    async fn verify(&self, request: VerifyRequest) -> Result<VerifyResponse, Error> {
        // Verify payment authorization
    }
    
    async fn settle(&self, request: SettleRequest) -> Result<SettleResponse, Error> {
        // Settle the payment on-chain
    }
}
```

### Timestamps

```rust
use x402_types::timestamp::UnixTimestamp;

// Create timestamp for payment validity window
let valid_after = UnixTimestamp::now();
let valid_before = valid_after + std::time::Duration::from_secs(3600);
```

## Related Crates

| Crate                                                                | Description                                    |
|----------------------------------------------------------------------|------------------------------------------------|
| [`x402-chain-eip155`](https://crates.io/x402-chain-eip155)           | EVM chain support (Ethereum, Base, etc.)       |
| [`x402-chain-solana`](https://crates.io/x402-chain-solana)           | Solana blockchain support                      |
| [`x402-chain-aptos`](https://crates.io/x402-chain-aptos)             | Aptos blockchain support                       |
| [`x402-axum`](https://crates.io/x402-axum)                           | Axum middleware for payment-gated endpoints    |
| [`x402-reqwest`](https://crates.io/x402-reqwest)                     | Reqwest client with automatic payment handling |
| [`x402-facilitator-local`](https://crates.io/x402-facilitator-local) | Local facilitator implementation               |

## Feature Flags

| Feature     | Description                                                     |
|-------------|-----------------------------------------------------------------|
| `cli`       | Enables CLI argument parsing via clap for configuration loading |
| `telemetry` | Enables tracing instrumentation for debugging and monitoring    |

## License

Apache-2.0
