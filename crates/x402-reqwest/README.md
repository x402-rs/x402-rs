# x402-reqwest

[![Crates.io](https://img.shields.io/crates/v/x402-reqwest.svg)](https://crates.io/crates/x402-reqwest)
[![Docs.rs](https://docs.rs/x402-reqwest/badge.svg)](https://docs.rs/x402-reqwest)

**Reqwest middleware that transparently handles HTTP `402 Payment Required` responses using the [x402 protocol](https://x402.org/).**

This crate enables your reqwest or reqwest-middleware-based HTTP clients to:
- Detect `402 Payment Required` responses
- Extract payment requirements from the response
- Sign payments using registered scheme clients
- Retry the request with the payment header attached

All in all: **automatically pay for resources using the x402 protocol**.

## Features

- Pluggable reqwest middleware using [reqwest-middleware](https://crates.io/crates/reqwest-middleware)
- Multi-chain support (EVM via EIP-155, Solana)
- Full V1 and V2 protocol support with automatic detection and handling
- Multi-scheme architecture supporting various payment schemes
- Customizable payment selection logic
- Tracing support (opt-in via `telemetry` feature)

## Installation

Add the dependency:

```toml
# Cargo.toml
x402-reqwest = "0.5"
```

To enable tracing:

```toml
x402-reqwest = { version = "0.5", features = ["telemetry"] }
```

## Quickstart

```rust,no_run
use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, X402Client};
use x402_rs::scheme::v1_eip155_exact::client::V1Eip155ExactClient;
use alloy_signer_local::PrivateKeySigner;
use std::sync::Arc;
use reqwest::Client;

let signer: Arc<PrivateKeySigner> = Arc::new("0x...".parse().unwrap());

// Create an X402 client and register scheme handlers
let x402_client = X402Client::new()
    .register(V1Eip155ExactClient::new(signer.clone()));

// Build a reqwest client with x402 middleware
let http_client = Client::new()
    .with_payments(x402_client)
    .build();

// Use the client - payments are handled automatically
let response = http_client
    .get("https://api.example.com/protected")
    .send()
    .await?;

println!("Status: {}", response.status());
```

## Registering Scheme Clients

The [`X402Client`] uses a plugin architecture for supporting different payment schemes.
Register scheme clients for each chain/network you want to support:

```rust,no_run
use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, X402Client};
use x402_rs::scheme::v1_eip155_exact::client::V1Eip155ExactClient;
use x402_rs::scheme::v2_eip155_exact::client::V2Eip155ExactClient;
use x402_rs::scheme::v1_solana_exact::client::V1SolanaExactClient;
use x402_rs::scheme::v2_solana_exact::client::V2SolanaExactClient;
use alloy_signer_local::PrivateKeySigner;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_keypair::Keypair;
use std::sync::Arc;
use reqwest::Client;

let evm_signer: Arc<PrivateKeySigner> = Arc::new("0x...".parse().unwrap());
let solana_keypair = Arc::new(Keypair::from_base58_string("..."));
let solana_rpc_client = Arc::new(RpcClient::new("https://api.devnet.solana.com"));

let x402_client = X402Client::new()
    // Register EVM schemes (V1 and V2)
    .register(V1Eip155ExactClient::new(evm_signer.clone()))
    .register(V2Eip155ExactClient::new(evm_signer))
    // Register Solana schemes (V1 and V2)
    .register(V1SolanaExactClient::new(
        solana_keypair.clone(),
        solana_rpc_client.clone(),
    ))
    .register(V2SolanaExactClient::new(solana_keypair, solana_rpc_client));

let http_client = Client::new()
    .with_payments(x402_client)
    .build();
```

## How It Works

1. A request is made to a server
2. If a `402 Payment Required` response is received, the middleware:
   - Parses the Payment-Required response (V1 body or V2 header)
   - Finds registered scheme clients that can handle the payment
   - Selects the best matching payment option
   - Signs the payment using the scheme client
   - Retries the request with the payment header attached

## Payment Selection

When multiple payment options are available, the [`X402Client`] uses a [`PaymentSelector`]
to choose the best option. By default, it uses [`FirstMatch`] which selects the first
matching scheme.

You can implement custom selection logic:

```rust,ignore
use x402_reqwest::X402Client;
use x402_rs::proto::client::{PaymentSelector, PaymentCandidate};

struct MyCustomSelector;

impl PaymentSelector for MyCustomSelector {
    fn select(&self, candidates: &[PaymentCandidate]) -> Option<&PaymentCandidate> {
        // Custom selection logic
        candidates.first()
    }
}

let client = X402Client::new()
    .with_selector(MyCustomSelector);
```

## Optional Features

- `telemetry`: Enables tracing annotations for richer observability

Enable it via:
```toml
x402-reqwest = { version = "0.5", features = ["telemetry"] }
```

## Telemetry

When the `telemetry` feature is enabled, the middleware emits structured tracing events for key operations:

- **x402.reqwest.handle**: Span covering the entire middleware handling, including 402 detection and payment retry
- **x402.reqwest.next**: Span for the underlying HTTP request (both initial and retry)
- **x402.reqwest.make_payment_headers**: Span for payment header creation and signing
- **x402.reqwest.parse_payment_required**: Span for parsing 402 responses (V1 body or V2 header)

The telemetry includes:
- Payment version (V1 or V2)
- Selected scheme and network
- Request URLs and response status codes
- Payment parsing results

This integrates with any `tracing`-compatible subscriber. For OpenTelemetry export, see [x402-rs telemetry](https://docs.rs/x402-rs/latest/x402_rs/util/telemetry/index.html).

## Related Crates

- [x402-rs](https://crates.io/crates/x402-rs): Core x402 types, facilitator traits, helpers.
- [x402-axum](https://crates.io/crates/x402-axum): Axum middleware for accepting x402 payments.

## License

[Apache-2.0](LICENSE)
