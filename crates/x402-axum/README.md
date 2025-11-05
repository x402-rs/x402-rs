# x402-axum

[![Crates.io](https://img.shields.io/crates/v/x402-axum.svg)](https://crates.io/crates/x402-axum)
[![Docs.rs](https://docs.rs/x402-axum/badge.svg)](https://docs.rs/x402-axum)

Axum middleware for protecting routes with [x402 protocol](https://www.x402.org) payments.

This crate provides a drop-in `tower::Layer` that intercepts incoming requests,
validates `X-Payment` headers using a configured x402 facilitator,
and settles the payment before responding.

If no valid payment is provided, a `402 Payment Required` response is returned with details about accepted assets and amounts.

## Features

- Built for [Axum](https://github.com/tokio-rs/axum)
- Fluent builder API for composing payment requirements and prices
- Enforces on-chain payment before executing protected handlers
- Returns standards-compliant `402 Payment Required` responses
- Emits rich tracing spans with optional OpenTelemetry integration (`telemetry` feature)
- Compatible with any x402 facilitator (remote or in-process)

## Installation
Add to your `Cargo.toml`:

```toml
x402-axum = "0.6"
```

If you want to enable tracing and OpenTelemetry support, use the telemetry feature (make sure to register a tracing subscriber in your application):
```toml
x402-axum = { version = "0.6", features = ["telemetry"] }
```

## Specifying Prices

Prices in x402 are defined using the `PriceTag` struct. A `PriceTag` includes:

- _Asset_ (`asset`) — the ERC-20 token used for payment
- _Amount_ (`amount`) — the required token amount, either as an integer or a human-readable decimal
- _Recipient_ (`pay_to`) — the address that will receive the tokens

You can construct `PriceTag`s directly or use fluent builder helpers that simplify common flows.

### Asset

**Bring Your Own Token**

If you're integrating a custom token, define it using `TokenDeployment`. This includes token address, decimals, the network it lives on, and EIP-712 metadata (name/version):

```rust
use x402_rs::types::{TokenAsset, TokenDeployment, EvmAddress, TokenAssetEip712};
use x402_rs::network::Network;

let asset = TokenDeployment {
    asset: TokenAsset {
        address: "0x036CbD53842c5426634e7929541eC2318f3dCF7e".parse().unwrap(),
        network: Network::BaseSepolia,
    },
    decimals: 6,
    eip712: TokenAssetEip712 {
        name: "MyToken".into(),
        version: "1".into(),
    },
};
```

**Known tokens (like USDC)**

For common stablecoins like USDC, you can use the convenience struct `USDCDeployment`:

```rust
use x402_rs::network::{Network, USDCDeployment};

let asset = USDCDeployment::by_network(Network::BaseSepolia);
```

### Amount

**Human-Readable Amounts**

Use `.amount("0.025")` on asset to define a price using a string or a number.
This will be converted to the correct on-chain amount based on the asset’s decimals:

```rust
usdc.amount("0.025") // → 25000 for 0.025 USDC with 6 decimals 
```

**Raw Token Amounts**

If you already know the amount in base units (e.g. 25000 for 0.025 USDC with 6 decimals), use `.token_amount(...)`:

```rust
usdc.token_amount(25000)
```

This will use the value onchain verbatim.

### Recipient

Use `.pay_to(...)` to set the address that should receive the payment.

```rust
let price_tag = usdc.amount(0.025).pay_to("0xYourAddress").unwrap();
```

### Integrating with Middleware

Once you’ve created your PriceTag, pass it to the middleware:

```rust
let x402 = X402Middleware::try_from("https://x402.org/facilitator/").unwrap();
let usdc = USDCDeployment::by_network(Network::BaseSepolia);

let app = Router::new().route("/paid-content", get(handler).layer( 
    // To allow multiple options (e.g., USDC or another token), chain them: 
    x402
        .with_price_tag(usdc.amount("0.025").pay_to("0xYourAddress").unwrap())
        .or_price_tag(other_token.amount("0.035").pay_to("0xYourAddress").unwrap())
    ),
);
```

You can extract shared fields like the payment recipient, then vary prices per route:

```rust
let x402 = X402Middleware::try_from("https://x402.org/facilitator/").unwrap();
let asset = USDCDeployment::by_network(Network::BaseSepolia)
    .pay_to("0xYourAddress"); // Both /vip-content and /extra-vip-content are paid to 0xYourAddress

let app: Router = Router::new()
    .route(
        "/vip-content",
        get(my_handler).layer(x402.with_price_tag(asset.amount("0.025").unwrap())),
    )
    .route(
        "/extra-vip-content",
        get(my_handler).layer(x402.with_price_tag(asset.amount("0.25").unwrap())),
    );
```

## Example

```rust
use axum::{Router, routing::get, Json};
use x402_axum::X402Middleware;
use x402_axum::price::IntoPriceTag;
use x402_rs::network::{Network, USDCDeployment};
use http::StatusCode;
use serde_json::json;

#[tokio::main]
async fn main() {
  let x402 = X402Middleware::try_from("https://x402.org/facilitator/").unwrap();
  let usdc = USDCDeployment::by_network(Network::BaseSepolia)
    .pay_to("0xYourAddress");

  let app = Router::new().route(
    "/paid-content",
    get(handler).layer(
      x402.with_description("Access to /paid-content")
        .with_price_tag(usdc.amount(0.01).unwrap())
    ),
  ); 
  
  let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
    .await
    .expect("Failed to start server");
  println!("Listening on {}", listener.local_addr().unwrap());
  axum::serve(listener, app).await.unwrap();
}

async fn handler() -> (StatusCode, Json<serde_json::Value>) { 
  (StatusCode::OK, Json(json!({ "message": "Hello, payer!" })))
}
```

## HTTP Behavior

If no valid payment is included, the middleware responds with:

```json5
// HTTP/1.1 402 Payment Required
// Content-Type: application/json
{
  "error": "X-PAYMENT header is required",
  "accepts": [
    // Payment Requirements
  ],
  "x402Version": 1
}
```

## Configuring Input and Output Schemas

You can provide detailed metadata about your API endpoints using `with_input_schema()` and `with_output_schema()`. These schemas are embedded in the `PaymentRequirements.outputSchema` field and can be used by discovery services, documentation generators, or clients to understand your API.

### Input Schema

The input schema describes the expected request format, including HTTP method, query parameters, headers, and whether the endpoint is publicly discoverable:

```rust
use serde_json::json;

let x402 = X402Middleware::try_from("https://x402.org/facilitator/").unwrap();

let app = Router::new().route(
    "/api/weather",
    get(handler).layer(
        x402.with_description("Weather API")
            .with_input_schema(json!({
                "type": "http",
                "method": "GET",
                "discoverable": true,  // Endpoint appears in discovery services
                "queryParams": {
                    "location": {
                        "type": "string",
                        "description": "City name or coordinates",
                        "required": true
                    },
                    "units": {
                        "type": "string",
                        "enum": ["metric", "imperial"],
                        "default": "metric"
                    }
                }
            }))
            .with_price_tag(usdc.amount("0.001").unwrap())
    ),
);
```

### Output Schema

The output schema describes the response format:

```rust
let app = Router::new().route(
    "/api/weather",
    get(handler).layer(
        x402.with_output_schema(json!({
            "type": "object",
            "properties": {
                "temperature": { "type": "number", "description": "Current temperature" },
                "conditions": { "type": "string", "description": "Weather conditions" },
                "humidity": { "type": "number", "description": "Humidity percentage" }
            },
            "required": ["temperature", "conditions"]
        }))
        .with_price_tag(usdc.amount("0.001").unwrap())
    ),
);
```

### Discoverable vs Private Endpoints

You can control whether your endpoint appears in public discovery services by setting the `discoverable` flag:

```rust
// Public endpoint - will appear in x402 Bazaar
x402.with_input_schema(json!({
    "type": "http",
    "method": "GET",
    "discoverable": true,
    "description": "Public weather API"
}))

// Private endpoint - direct access only
x402.with_input_schema(json!({
    "type": "http",
    "method": "GET",
    "discoverable": false,
    "description": "Internal admin API - private access only"
}))
```

The combined input and output schemas are automatically embedded in `PaymentRequirements.outputSchema` as:

```json
{
  "input": { /* your input schema */ },
  "output": { /* your output schema */ }
}
```

## Optional Telemetry

If the `telemetry` feature is enabled, the middleware emits structured tracing spans such as:
- `x402.handle_request`,
- `x402.verify_payment`,
- `x402.settle_payment`,

You can connect these to OpenTelemetry exporters like Jaeger, Tempo, or Otel Collector.

To enable:

```toml
[dependencies]
x402-axum = { version = "0.6", features = ["telemetry"] }
```

## Related Crates	
- [x402-rs](https://crates.io/crates/x402-rs): Core x402 types, facilitator traits, helpers.

## License

[Apache-2.0](LICENSE)
