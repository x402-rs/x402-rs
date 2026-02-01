# Build Your Own Facilitator

This guide explains how to build a custom x402 facilitator implementation using the x402-rs ecosystem.

## Overview

A facilitator is a service that:
- **Verifies** payment payloads signed by clients
- **Settles** payments on-chain
- **Manages** blockchain connections and signers

The x402-rs ecosystem provides building blocks to create custom facilitators tailored to your needs.

## Architecture

The facilitator architecture consists of:

```
┌─────────────────────────────────────────┐
│         Your HTTP Server                │
│      (Axum, Actix, Rocket, etc.)        │
└────────────────┬────────────────────────┘
                 │
┌────────────────▼────────────────────────┐
│    x402-facilitator-local               │
│  (Verification & Settlement Logic)      │
└────────────────┬────────────────────────┘
                 │
     ┌────────────┴────────────┐
     │                         │
┌───▼────────┐        ┌──────▼──────┐
│ Chain      │        │ Scheme      │
│ Registry   │        │ Registry    │
└───┬────────┘        └──────┬──────┘
    │                        │
    ├─ EIP-155 Provider      ├─ V1Eip155Exact
    ├─ Solana Provider       ├─ V2Eip155Exact
    └─ Aptos Provider        ├─ V1SolanaExact
                             ├─ V2SolanaExact
                             └─ V2AptosExact
```

## Getting Started

### 1. Add Dependencies

> **Note:** The versions shown below are indicative. Please check the latest versions on [crates.io](https://crates.io) or the source repository if the packages are not published on crates.io.

```toml
[dependencies]
x402-types = { version = "1.0", features = ["cli"] }
x402-facilitator-local = { version = "1.0" }
x402-chain-eip155 = { version = "1.0", features = ["facilitator"] }
x402-chain-solana = { version = "1.0", features = ["facilitator"] }

dotenvy = "0.15"
serde_json = "1.0"
tokio = { version = "1.35", features = ["full"] }
async-trait = "0.1"
axum = "0.8"
tower-http = "0.9"
rustls = { version = "0.23", features = ["ring"] }
```

### 2. Initialize the Facilitator

```rust
use x402_facilitator_local::{FacilitatorLocal, handlers};
use x402_types::chain::{ChainRegistry, FromConfig};
use x402_types::scheme::{SchemeBlueprints, SchemeRegistry};
use x402_chain_eip155::{V1Eip155Exact, V2Eip155Exact};
use x402_chain_solana::{V1SolanaExact, V2SolanaExact};
use std::sync::Arc;
use axum::Router;
use tower_http::cors;
use axum::http::Method;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize rustls crypto provider
    rustls::crypto::CryptoProvider::install_default(
        rustls::crypto::ring::default_provider()
    ).expect("Failed to initialize rustls crypto provider");

    // Load .env variables
    dotenvy::dotenv().ok();

    // Load configuration
    let config = Config::load()?;

    // Initialize chain registry from config
    let chain_registry = ChainRegistry::from_config(config.chains()).await?;

    // Register supported schemes
    let scheme_blueprints = {
        let mut blueprints = SchemeBlueprints::new();
        blueprints.register(V1Eip155Exact);
        blueprints.register(V2Eip155Exact);
        blueprints.register(V1SolanaExact);
        blueprints.register(V2SolanaExact);
        blueprints
    };

    // Build scheme registry
    let scheme_registry =
        SchemeRegistry::build(chain_registry, scheme_blueprints, config.schemes());

    // Create facilitator
    let facilitator = FacilitatorLocal::new(scheme_registry);
    let state = Arc::new(facilitator);

    // Create HTTP routes with CORS
    let app = Router::new()
        .merge(handlers::routes().with_state(state))
        .layer(
            cors::CorsLayer::new()
                .allow_origin(cors::Any)
                .allow_methods([Method::GET, Method::POST])
                .allow_headers(cors::Any),
        );

    // Run server
    let addr = SocketAddr::new(config.host(), config.port());
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
```

### 3. Configuration

Create a `config.json` file:

```json
{
  "port": 8080,
  "host": "0.0.0.0",
  "chains": {
    "eip155:8453": {
      "eip1559": true,
      "flashblocks": true,
      "signers": ["$FACILITATOR_PRIVATE_KEY"],
      "rpc": [
        {
          "http": "https://mainnet.base.org",
          "rate_limit": 100
        }
      ]
    },
    "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp": {
      "signer": "$SOLANA_PRIVATE_KEY",
      "rpc": "https://api.mainnet-beta.solana.com",
      "pubsub": "wss://api.mainnet-beta.solana.com"
    }
  },
  "schemes": [
    {
      "id": "v1-eip155-exact",
      "chains": "eip155:*"
    },
    {
      "id": "v2-eip155-exact",
      "chains": "eip155:*"
    },
    {
      "id": "v1-solana-exact",
      "chains": "solana:*"
    },
    {
      "id": "v2-solana-exact",
      "chains": "solana:*"
    }
  ]
}
```

## Advanced Customization

### Custom Scheme Implementation

To implement a custom payment scheme:

1. Implement the `X402SchemeFacilitator` trait from `x402-types`
2. Implement the `X402SchemeFacilitatorBuilder` trait
3. Implement the `X402SchemeId` trait
4. Register it with the `SchemeBlueprints`

See the [How to Write a Scheme](./how-to-write-a-scheme.md) guide for detailed instructions.

### Custom Chain Support

To add support for a new blockchain:

1. Implement the `ChainProviderOps` trait for your provider type
2. Implement the `FromConfig` trait to construct your provider from configuration
3. Create scheme implementations for your chain
4. Register with the `ChainRegistry`

### Middleware Integration

Integrate with your existing HTTP framework:

```rust
use axum::{middleware, Router};

let app = Router::new()
    .route("/verify", post(verify_handler))
    .route("/settle", post(settle_handler))
    .layer(middleware::from_fn(your_auth_middleware));
```

## Deployment

### Docker

Create a `Dockerfile`:

```dockerfile
FROM rust:1.88 as builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM debian:bookworm-slim
COPY --from=builder /app/target/release/your-facilitator /usr/local/bin/
ENTRYPOINT ["your-facilitator"]
```

### Environment Variables

- `HOST` - Server bind address (default: `0.0.0.0`)
- `PORT` - Server port (default: `8080`)
- `CONFIG` - Path to configuration file (default: `config.json`)
- `RUST_LOG` - Log level (default: `info`)
- `OTEL_EXPORTER_OTLP_ENDPOINT` - OpenTelemetry collector endpoint

## Observability

Enable OpenTelemetry tracing:

```rust
use x402_facilitator_local::util::Telemetry;

let telemetry = Telemetry::new()
    .with_name("my-facilitator")
    .with_version("1.0.0")
    .register();

let tracing_layer = telemetry.http_tracing();

let app = Router::new()
    .merge(handlers::routes().with_state(state))
    .layer(tracing_layer);
```

## Examples

- [x402-facilitator](../facilitator) - Production-ready facilitator binary
- [x402-axum-example](../examples/x402-axum-example) - Server example
- [x402-reqwest-example](../examples/x402-reqwest-example) - Client example

## Support

For questions or issues:
- Open an issue on [GitHub](https://github.com/x402-rs/x402-rs)
- Check the [x402 protocol documentation](https://x402.org)
- Review individual crate documentation on [docs.rs](https://docs.rs)
