# x402-facilitator-local

[![Crates.io](https://img.shields.io/crates/v/x402-facilitator-local.svg)](https://crates.io/crates/x402-facilitator-local)
[![Docs.rs](https://docs.rs/x402-facilitator-local/badge.svg)](https://docs.rs/x402-facilitator-local)

Local facilitator implementation for the [x402](https://www.x402.org) payment protocol.

This crate provides a self-hosted facilitator that validates x402 payment payloads and performs on-chain settlements using registered scheme handlers. It includes HTTP handlers for the x402 protocol endpoints and utilities for graceful shutdown and OpenTelemetry tracing.

## Features

- **Local Facilitator**: [`FacilitatorLocal`] implementation that delegates to scheme handlers
- **HTTP Handlers**: Axum-based endpoints for `/verify`, `/settle`, `/supported`, and `/health`
- **Multi-chain Support**: Works with any chain implementation (EIP-155, Solana, Aptos)
- **Scheme Registry**: Pluggable architecture for supporting multiple payment schemes
- **Graceful Shutdown**: Signal handling for clean server shutdown
- **OpenTelemetry**: Optional tracing and metrics support (`telemetry` feature)

## Installation

Add to your `Cargo.toml`:

```toml
x402-facilitator-local = "0.1"
```

With telemetry support:

```toml
x402-facilitator-local = { version = "0.1", features = ["telemetry"] }
```

## Usage

### Basic Setup

```rust
use x402_facilitator_local::{FacilitatorLocal, handlers};
use x402_types::chain::ChainRegistry;
use x402_types::scheme::{SchemeBlueprints, SchemeRegistry};
use x402_chain_eip155::{V1Eip155Exact, V2Eip155Exact};
use x402_chain_solana::{V1SolanaExact, V2SolanaExact};
use x402_chain_aptos::V2AptosExact;
use std::sync::Arc;
use axum::Router;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize chain registry from configuration
    let chain_registry = ChainRegistry::from_config(&chains_config).await?;
    
    // Register supported schemes
    let scheme_blueprints = SchemeBlueprints::new()
        .and_register(V1Eip155Exact)
        .and_register(V2Eip155Exact)
        .and_register(V1SolanaExact)
        .and_register(V2SolanaExact)
        .and_register(V2AptosExact);
    
    // Build the scheme registry
    let scheme_registry = SchemeRegistry::build(
        chain_registry,
        scheme_blueprints,
        &schemes_config,
    );
    
    // Create the local facilitator
    let facilitator = FacilitatorLocal::new(scheme_registry);
    let state = Arc::new(facilitator);
    
    // Create HTTP routes
    let app = Router::new()
        .merge(handlers::routes().with_state(state));
    
    // Run the server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app).await?;
    
    Ok(())
}
```

### With Graceful Shutdown

```rust
use x402_facilitator_local::util::SigDown;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // ... setup facilitator and routes ...
    
    let sig_down = SigDown::try_new()?;
    let cancellation_token = sig_down.cancellation_token();
    
    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            cancellation_token.cancelled().await;
        })
        .await?;
    
    Ok(())
}
```

### With OpenTelemetry

```rust
use x402_facilitator_local::util::Telemetry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize telemetry (reads from OTEL_* environment variables)
    let telemetry = Telemetry::new()
        .with_name("x402-facilitator")
        .with_version("1.0.0")
        .register();
    
    // Get the HTTP tracing layer
    let tracing_layer = telemetry.http_tracing();
    
    // ... setup facilitator and routes ...
    let app = Router::new()
        .merge(handlers::routes().with_state(state))
        .layer(tracing_layer);
    
    // Run server...
    Ok(())
}
```

## HTTP Endpoints

The [`handlers`] module provides the following endpoints:

| Endpoint     | Method | Description                                 |
|--------------|--------|---------------------------------------------|
| `/`          | GET    | Simple greeting message                     |
| `/verify`    | GET    | Schema information for verify endpoint      |
| `/verify`    | POST   | Verify a payment payload                    |
| `/settle`    | GET    | Schema information for settle endpoint      |
| `/settle`    | POST   | Settle a verified payment on-chain          |
| `/supported` | GET    | List supported payment schemes and networks |
| `/health`    | GET    | Health check (delegates to `/supported`)    |

## Architecture

The local facilitator uses a scheme-based architecture:

1. **Chain Registry**: Manages blockchain providers and connections
2. **Scheme Blueprints**: Defines available payment schemes (V1/V2, EIP-155/Solana/Aptos)
3. **Scheme Registry**: Combines chains and schemes into executable handlers
4. **FacilitatorLocal**: Routes requests to the appropriate scheme handler

```text
┌─────────────────┐
│ FacilitatorLocal│
└────────┬────────┘
         │
    ┌────▼────┐
    │ Scheme  │
    │Registry │
    └────┬────┘
         │
    ┌────┴────┐
    ▼         ▼
┌───────┐ ┌───────┐
│V2Eip  │ │V2Sol  │
│155Exact│ │anaExact│
└───┬───┘ └───┬───┘
    │         │
┌───▼───┐ ┌───▼───┐
│Eip155 │ │Solana │
│Provider│ │Provider│
└───────┘ └───────┘
```

## Configuration

The facilitator requires configuration for chains and optionally for schemes:

```json
{
  "chains": {
    "eip155:8453": {
      "eip1559": true,
      "signers": ["$FACILITATOR_PRIVATE_KEY"],
      "rpc": [
        {
          "http": "https://mainnet.base.org",
          "rate_limit": 100
        }
      ]
    },
    "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp": {
      "signers": ["$SOLANA_PRIVATE_KEY"],
      "rpc": [
        {
          "http": "https://api.mainnet-beta.solana.com"
        }
      ]
    }
  },
  "schemes": [
    {
      "scheme": "v2-eip155-exact",
      "chains": ["eip155:8453"]
    },
    {
      "scheme": "v2-solana-exact",
      "chains": ["solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"]
    }
  ]
}
```

## Feature Flags

| Feature     | Description                               |
|-------------|-------------------------------------------|
| `telemetry` | Enables OpenTelemetry tracing and metrics |

## Environment Variables

When using the `telemetry` feature:

| Variable                      | Description                          |
|-------------------------------|--------------------------------------|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OTLP collector endpoint              |
| `OTEL_EXPORTER_OTLP_PROTOCOL` | Protocol (`http/protobuf` or `grpc`) |
| `OTEL_SERVICE_NAME`           | Service name for traces              |
| `OTEL_SERVICE_VERSION`        | Service version                      |
| `OTEL_SERVICE_DEPLOYMENT`     | Deployment environment               |

## Related Crates

| Crate                                                             | Description                                    |
|-------------------------------------------------------------------|------------------------------------------------|
| [`x402-types`](https://crates.io/crates/x402-types)               | Core types and facilitator trait               |
| [`x402-chain-eip155`](https://crates.io/crates/x402-chain-eip155) | EIP-155 (EVM) chain support                    |
| [`x402-chain-solana`](https://crates.io/crates/x402-chain-solana) | Solana chain support                           |
| [`x402-chain-aptos`](https://crates.io/crates/x402-chain-aptos)   | Aptos chain support                            |
| [`x402-axum`](https://crates.io/crates/x402-axum)                 | Axum middleware for payment-gated endpoints    |
| [`x402-reqwest`](https://crates.io/crates/x402-reqwest)           | Reqwest client with automatic payment handling |

## License

Apache-2.0
