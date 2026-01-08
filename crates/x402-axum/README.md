# x402-axum

[![Crates.io](https://img.shields.io/crates/v/x402-axum.svg)](https://crates.io/crates/x402-axum)
[![Docs.rs](https://docs.rs/x402-axum/badge.svg)](https://docs.rs/x402-axum)

<div align="center">
<table><tr><td>
🔧 <strong>Protocol v2 Update Coming</strong> — This crate is being updated to support x402 protocol v2 with the new multi-chain, multi-scheme architecture. The update is on its way! For v1 support, see the <code>protocol-x402-v1</code> branch.
</td></tr></table>
</div>

**Axum middleware for protecting routes with [x402 protocol](https://www.x402.org) payments.**

This crate provides a drop-in `tower::Layer` that intercepts incoming requests,
validates payment headers using a configured x402 facilitator,
and settles the payment before or after request execution (configurable).

If no valid payment is provided, a `402 Payment Required` response is returned with details about accepted assets and amounts.

## Features

- Built for [Axum](https://github.com/tokio-rs/axum)
- Supports both V1 and V2 x402 protocols
- Fluent builder API for configuring payment requirements
- Configurable settlement timing (before or after request execution)
- Returns standards-compliant `402 Payment Required` responses
- Emits rich tracing spans with optional OpenTelemetry integration (`telemetry` feature)
- Compatible with any x402 facilitator
- Configurable facilitator cache TTL for performance optimization

## Installation

Add to your `Cargo.toml`:

```toml
x402-axum = "0.7"
```

If you want to enable tracing and OpenTelemetry support, use the telemetry feature:

```toml
x402-axum = { version = "0.7", features = ["telemetry"] }
```

## Quickstart

```rust,no_run
use axum::{Router, routing::get};
use axum::response::IntoResponse;
use http::StatusCode;
use x402_axum::X402Middleware;
use x402_rs::networks::{KnownNetworkEip155, USDC};
use x402_rs::scheme::v1_eip155_exact::V1Eip155Exact;

let x402 = X402Middleware::new("https://facilitator.x402.rs");

let app: Router = Router::new().route(
    "/protected",
    get(my_handler).layer(
        x402.with_price_tag(V1Eip155Exact::price_tag(
            "0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07".parse().unwrap(),
            USDC::base_sepolia().amount(10),
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

## Defining Prices

Prices are defined using the scheme-specific price tag types from `x402_rs`. The crate includes
built-in schemes for common protocols:

- **[`V1Eip155Exact::price_tag()`]** - V1 EIP-155 exact payment on EVM chains
- **[`V2Eip155Exact::price_tag()`]** - V2 EIP-155 exact payment on EVM chains
- **[`V1SolanaExact::price_tag()`]** - V1 Solana exact payment
- **[`V2SolanaExact::price_tag()`]** - V2 Solana exact payment

### Built-in Schemes

```rust,no_run
use x402_axum::X402Middleware;
use x402_rs::networks::{KnownNetworkEip155, KnownNetworkSolana, USDC};
use x402_rs::scheme::v1_eip155_exact::V1Eip155Exact;
use x402_rs::scheme::v1_solana_exact::V1SolanaExact;

let x402 = X402Middleware::new("https://facilitator.x402.rs");

// Accept both EVM and Solana payments
let app = Router::new().route(
    "/premium",
    get(handler).layer(
        x402.with_price_tag(V1Eip155Exact::price_tag(
            "0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07".parse().unwrap(),
            USDC::base_sepolia().amount(10),
        )).with_price_tag(V1SolanaExact::price_tag(
            "EGBQqKn968sVv5cQh5Cr72pSTHfxsuzq7o7asqYB5uEV".parse().unwrap(),
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

1. **Define your price tag type** - A struct that holds payment parameters (recipient, amount, asset)
2. **Implement [`PaygateProtocol`]** - Handle verification, error responses, and facilitator enrichment
3. **Implement [`IntoPriceTag`]** - Allow converting from your application types to price tags

Example structure for a custom scheme:

```rust
use axum_core::response::Response;
use x402_axum::paygate::{PaygateError, PaygateProtocol, ResourceInfoBuilder, VerificationError};
use x402_rs::proto::{self, v2, SupportedResponse};
use x402_rs::scheme::IntoPriceTag;

pub struct MyCustomPriceTag {
    pub recipient: String,
    pub amount: String,
    pub asset: String,
}

// Implement PaygateProtocol for your custom price tag
impl PaygateProtocol for MyCustomPriceTag {
    type PaymentPayload = /* your payload type */;

    /// Use `X-PAYMENT` for V1 protocol or `Payment-Signature` for V2 protocol
    const PAYMENT_HEADER_NAME: &'static str = "Payment-Signature";

    fn make_verify_request(
        payload: Self::PaymentPayload,
        accepts: &[Self],
        resource: &v2::ResourceInfo,
    ) -> Result<proto::VerifyRequest, VerificationError> {
        // Convert your price tag + payload into a verify request
        todo!()
    }

    fn error_into_response(
        err: PaygateError,
        accepts: &[Self],
        resource: &v2::ResourceInfo,
    ) -> Response {
        // Format errors according to your scheme's protocol
        todo!()
    }

    fn validate_verify_response(
        verify_response: proto::VerifyResponse,
    ) -> Result<(), VerificationError> {
        // Validate the facilitator's response
        todo!()
    }

    fn enrich_with_capabilities(
        price_tag: &Self,
        capabilities: &SupportedResponse,
    ) -> Self {
        // Add facilitator-specific data (e.g., fee payers)
        price_tag.clone()
    }
}

// Implement IntoPriceTag for conversion
impl IntoPriceTag for MyCustomPriceTag {
    type PriceTag = Self;

    fn into_price_tag(self) -> Self::PriceTag {
        self
    }
}
```

For a complete example, see the [How to Write a Scheme](docs/how-to-write-a-scheme.md) guide.

## Settlement Timing

By default, the middleware settles payments **after** request execution. You can change this:

```rust
let x402 = X402Middleware::new("https://facilitator.x402.rs")
    .settle_before_execution();  // Settle before executing the handler
```

Settling before execution is useful when you want to:
- Avoid failed settlements requiring external retry mechanisms
- Prevent payment authorization expiration before final settlement
- Ensure payment is settled before granting access to the resource

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
use url::Url;

let app = Router::new().route(
    "/premium-content",
    get(handler).layer(
        x402.with_price_tag(V1Eip155Exact::price_tag(
            recipient,
            USDC::base_sepolia().amount(10),
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

- [x402-rs](https://crates.io/crates/x402-rs): Core x402 types, facilitator traits, helpers.
- [x402-reqwest](https://crates.io/crates/x402-reqwest): Reqwest middleware for paying x402 requests.

## License

[Apache-2.0](LICENSE)
