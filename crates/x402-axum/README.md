# x402-axum

[![Crates.io](https://img.shields.io/crates/v/x402-axum.svg)](https://crates.io/crates/x402-axum)
[![Docs.rs](https://docs.rs/x402-axum/badge.svg)](https://docs.rs/x402-axum)

**Axum middleware for protecting routes with [x402 protocol](https://www.x402.org) payments.**

This crate provides a drop-in `tower::Layer` that intercepts incoming requests,
validates payment headers using a configured x402 facilitator,
and settles the payment before or after request execution (configurable).

If no valid payment is provided, a `402 Payment Required` response is returned with details about accepted assets and amounts.

## Features

- Built for [Axum](https://github.com/tokio-rs/axum)
- Full Protocol v2 Support - Complete implementation of x402 protocol v2
- Multi-chain Support - EVM (EIP-155), Solana, and Aptos chains
- Multi-scheme Architecture - Support for various payment schemes
- Fluent builder API for configuring payment requirements
- Configurable settlement timing (before or after request execution)
- Returns standards-compliant `402 Payment Required` responses
- Emits rich tracing spans with optional OpenTelemetry integration (`telemetry` feature)
- Compatible with any x402 facilitator
- Configurable facilitator cache TTL for performance optimization

## Installation

Add to your `Cargo.toml`:

```toml
x402-axum = "0.8"
```

If you want to enable tracing and OpenTelemetry support, use the telemetry feature:

```toml
x402-axum = { version = "0.8", features = ["telemetry"] }
```

## Quickstart

```rust,no_run
use alloy_primitives::address;
use axum::{Router, routing::get};
use axum::response::IntoResponse;
use http::StatusCode;
use x402_axum::X402Middleware;
use x402_chain_eip155::{KnownNetworkEip155, V1Eip155Exact};
use x402_types::networks::USDC;

let x402 = X402Middleware::new("https://facilitator.x402.rs");

let app: Router = Router::new().route(
    "/protected",
    get(my_handler).layer(
        x402.with_price_tag(V1Eip155Exact::price_tag(
            address!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07"),
            USDC::base_sepolia().parse("0.01").unwrap(),
        ))
    ),
);

async fn my_handler() -> impl IntoResponse {
    (StatusCode::OK, "This is VIP content!")
}
```

## Settlement Timing

By default, the middleware settles payments **after** request execution. You can change this:

```rust
let x402 = X402Middleware::new("https://facilitator.x402.rs")
    .settle_before_execution();  // Settle before executing the handler
```

Or explicitly set settlement after execution (default behavior):

```rust
let x402 = X402Middleware::new("https://facilitator.x402.rs")
    .settle_after_execution();  // Settle after successful request execution
```

## Dynamic Pricing

The middleware supports dynamic pricing through the `with_dynamic_price` method, which allows you to compute prices per-request based on headers, URI, or other runtime factors:

```rust,no_run
use alloy_primitives::address;
use axum::Router;
use axum::routing::get;
use x402_axum::X402Middleware;
use x402_chain_eip155::{KnownNetworkEip155, V2Eip155Exact};
use x402_types::networks::USDC;

let x402 = X402Middleware::new("https://facilitator.x402.rs");

let app = Router::new().route(
    "/api/data",
    get(handler).layer(x402.with_dynamic_price(|_headers, uri, _base_url| {
        // Check for discount query parameter
        let has_discount = uri.query().map(|q| q.contains("discount")).unwrap_or(false);
        let amount = if has_discount { 50 } else { 100 };

        async move {
            vec![V2Eip155Exact::price_tag(
                address!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07"),
                USDC::base_sepolia().amount(amount),
            )]
        }
    })),
);
```

### Conditional Free Access (Empty Price Tags)

When the dynamic pricing callback returns an **empty vector**, the middleware bypasses payment enforcement entirely and forwards the request directly to the handler. This is useful for implementing:

- Free tiers or promotional access
- Conditional pricing based on user authentication
- A/B testing with paid vs free access

```rust,no_run
use alloy_primitives::address;
use axum::Router;
use axum::routing::get;
use x402_axum::X402Middleware;
use x402_chain_eip155::{KnownNetworkEip155, V2Eip155Exact};
use x402_types::networks::USDC;

let x402 = X402Middleware::new("https://facilitator.x402.rs");

let app = Router::new().route(
    "/api/data",
    get(handler).layer(x402.with_dynamic_price(|_headers, uri, _base_url| {
        // Check if "free" query parameter is present
        let is_free = uri.query().map(|q| q.contains("free")).unwrap_or(false);

        async move {
            if is_free {
                // Return empty vector to bypass payment enforcement
                vec![]
            } else {
                // Normal pricing - payment required
                vec![V2Eip155Exact::price_tag(
                    address!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07"),
                    USDC::base_sepolia().amount(100),
                )]
            }
        }
    })),
);
```

With this configuration:
- `GET /api/data` → Returns 402 Payment Required
- `GET /api/data?free` → Bypasses payment, returns content directly

## Defining Prices

Prices are defined using the scheme-specific price tag types from the chain-specific crates. The following
built-in schemes are available:

- **`V1Eip155Exact::price_tag()`** - V1 EIP-155 exact payment on EVM chains (ERC-3009)
- **`V2Eip155Exact::price_tag()`** - V2 EIP-155 exact payment on EVM chains (ERC-3009)
- **`V1SolanaExact::price_tag()`** - V1 Solana exact payment (SPL token transfer)
- **`V2SolanaExact::price_tag()`** - V2 Solana exact payment (SPL token transfer)
- **`V2AptosExact::price_tag()`** - V2 Aptos exact payment (Coin transfer)

### Built-in Schemes

```rust,no_run
use alloy_primitives::address;
use axum::Router;
use axum::routing::get;
use solana_pubkey::pubkey;
use x402_axum::X402Middleware;
use x402_chain_eip155::{KnownNetworkEip155, V1Eip155Exact};
use x402_chain_solana::{KnownNetworkSolana, V1SolanaExact};
use x402_types::networks::USDC;

let x402 = X402Middleware::new("https://facilitator.x402.rs");

// Accept both EVM and Solana payments
let app = Router::new().route(
    "/premium",
    get(handler).layer(
        x402.with_price_tag(V1Eip155Exact::price_tag(
            address!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07"),
            USDC::base_sepolia().parse("0.01").unwrap(),
        )).with_price_tag(V1SolanaExact::price_tag(
            pubkey!("EGBQqKn968sVv5cQh5Cr72pSTHfxsuzq7o7asqYB5uEV"),
            USDC::solana().amount(100),
        ))
    ),
);
```

### Custom Schemes

You can implement custom payment schemes by implementing the [`PaygateProtocol`] trait from
`x402_axum::paygate`. This allows you to support additional blockchains, payment mechanisms,
or otherwise custom schemes.

To create a custom scheme, you'll need to:

1. **Define your scheme struct** - A unit struct that serves as a namespace for your scheme
2. **Add a `price_tag` method** - A static method that constructs the protocol-specific price tag
3. **Implement [`PaygateProtocol`]** - Handle verification, error responses, and facilitator enrichment

Example structure for a custom scheme:

```rust
use axum_core::response::Response;
use x402_axum::paygate::{PaygateError, PaygateProtocol, ResourceInfoBuilder, VerificationError};
use x402_types::proto::{self, v2, SupportedResponse};

/// Your custom scheme struct
pub struct MyCustomScheme;

impl MyCustomScheme {
    /// Create a price tag for this scheme
    pub fn price_tag(
        pay_to: String,
        asset: String,
        amount: u64,
    ) -> v2::PriceTag {
        v2::PriceTag {
            requirements: v2::PaymentRequirements {
                scheme: "my-custom-scheme".to_string(),
                pay_to,
                asset,
                network: /* your chain id */,
                amount: amount.to_string(),
                max_timeout_seconds: 300,
                extra: None,
            },
            enricher: None, // Or Some(Arc::new(your_enricher_fn)) if needed
        }
    }
}

// Implement PaygateProtocol for the price tag type (v2::PriceTag in this case)
// Note: PaygateProtocol is already implemented for v1::PriceTag and v2::PriceTag
// You only need to implement it if you're creating a completely custom price tag type
```

For a complete example, see the [How to Write a Scheme](../../docs/how-to-write-a-scheme.md) guide.

## Configuration

### Base URL

Set a base URL for computing resource URLs dynamically:

```rust
use url::Url;

let x402 = X402Middleware::new("https://facilitator.x402.rs")
    .with_base_url(Url::parse("https://api.example.com").unwrap());
```

### Resource URL

Set an explicit resource URL (recommended in production):

```rust
use alloy_primitives::address;
use axum::Router;
use axum::routing::get;
use url::Url;
use x402_chain_eip155::V1Eip155Exact;
use x402_types::networks::USDC;

let app = Router::new().route(
    "/premium-content",
    get(handler).layer(
        x402.with_price_tag(V1Eip155Exact::price_tag(
            address!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07"),
            USDC::base_sepolia().parse("0.01").unwrap(),
        )).with_resource(Url::parse("https://api.example.com/premium-content").unwrap())
    ),
);
```

### Description and MIME Type

```rust
let app = Router::new().route(
    "/api/data",
    get(handler).layer(
        x402.with_price_tag(price_tag)
            .with_description("Access to premium API")
            .with_mime_type("application/json")
    ),
);
```

### Facilitator Cache TTL

Configure the TTL for caching the facilitator's supported response:

```rust
use std::time::Duration;

let x402 = X402Middleware::new("https://facilitator.x402.rs")
    .with_supported_cache_ttl(Duration::from_secs(300)); // 5 minutes
```

To disable caching entirely:

```rust
let x402 = X402Middleware::new("https://facilitator.x402.rs")
    .with_supported_cache_ttl(Duration::from_secs(0));
```

## HTTP Behavior

If no valid payment is included, the middleware responds with a 402 Payment Required:

**V1 Protocol:**
```json
// HTTP/1.1 402 Payment Required
// Content-Type: application/json
{
  "error": "X-PAYMENT header is required",
  "accepts": [...],
  "x402Version": "1"
}
```

**V2 Protocol:**
```
// HTTP/1.1 402 Payment Required
// Payment-Required: <base64-encoded PaymentRequired>
```

## Error Handling

The middleware provides detailed error information through the `VerificationError` and `PaygateError` types:

- `VerificationError::PaymentHeaderRequired`: Missing payment header
- `VerificationError::InvalidPaymentHeader`: Malformed payment header
- `VerificationError::NoPaymentMatching`: No matching payment requirements found
- `VerificationError::VerificationFailed`: Payment verification failed
- `PaygateError::Settlement`: Payment settlement failed

These errors are automatically converted to appropriate 402 Payment Required responses with detailed error messages.

## Optional Telemetry

If the `telemetry` feature is enabled, the middleware emits structured tracing spans:
- `x402.handle_request`
- `x402.verify_payment`
- `x402.settle_payment`

You can connect these to OpenTelemetry exporters like Jaeger, Tempo, or Otel Collector.

## Related Crates

- [x402-types](https://crates.io/crates/x402-types): Core x402 types, facilitator traits, protocol definitions.
- [x402-reqwest](https://crates.io/crates/x402-reqwest): Reqwest middleware for paying x402 requests.
- [x402-chain-eip155](https://crates.io/crates/x402-chain-eip155): EVM/EIP-155 chain support for x402.
- [x402-chain-solana](https://crates.io/crates/x402-chain-solana): Solana chain support for x402.
- [x402-chain-aptos](https://crates.io/crates/x402-chain-aptos): Aptos chain support for x402.

## License

[Apache-2.0](LICENSE)
