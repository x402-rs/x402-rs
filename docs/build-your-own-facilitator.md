# Build Your Own Facilitator

This guide explains how to build a custom x402 facilitator implementation using the x402-rs ecosystem.

## Overview

A facilitator is a service that:
- **Verifies** payment payloads signed by clients
- **Settles** payments on-chain
- **Manages** blockchain connections and signers

The x402-rs ecosystem provides building blocks to create custom facilitators tailored to your needs.

## Why Build a Custom Facilitator?

You might want to build a custom facilitator for several reasons:

1. **Support for custom blockchains** — You need to support a blockchain that is not yet supported by the official x402-rs crates. This involves implementing a custom chain provider and adapting the payment schemes to work with it.

2. **Custom chain provider behavior** — You want to customize how the facilitator interacts with a supported chain. For example, you might want to implement a custom `Eip155MetaTransactionProvider` for EVM chains to add custom transaction signing logic, gas pricing strategies, or nonce management.

3. **Chain-specific deployment** — You want to run a facilitator that only supports specific chains or schemes, reducing the binary size and attack surface. This can be achieved through feature flags in the `facilitator` crate or by creating a minimal custom facilitator.

4. **Custom middleware or authentication** — You need to add custom HTTP middleware, authentication, or logging that is specific to your infrastructure.

5. **Integration with existing infrastructure** — You want to integrate the x402 facilitator into an existing application or service, rather than running it as a standalone binary.

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

To add support for a new blockchain that is not yet supported by x402-rs:

1. **Implement the `ChainProviderOps` trait** for your provider type. This trait provides basic operations like getting signer addresses and chain ID.

2. **Implement the `FromConfig` trait** to construct your provider from configuration. This allows your provider to be initialized from the JSON configuration file.

3. **Create scheme implementations for your chain**. For each scheme you want to support (e.g., `exact`), implement the `X402SchemeFacilitator` trait. Your scheme will use your custom chain provider to interact with the blockchain.

4. **Register with the `ChainRegistry`**. Add your chain provider to the registry so it can be discovered by the scheme registry.

5. **Add the scheme to your facilitator's `schemes.rs`**. Similar to how the `facilitator` crate has a `schemes.rs` file that implements `X402SchemeFacilitatorBuilder` for each scheme, you'll need to add your scheme there to bridge the generic `ChainProvider` enum to your chain-specific provider type.

### Custom Chain Provider for Supported Chains

Even for supported chains like EIP-155 (EVM), you might want to customize the chain provider behavior. The EIP-155 schemes use the `Eip155MetaTransactionProvider` trait to send transactions. You can implement this trait to customize:

- **Transaction signing logic** — Add custom signature validation or multi-sig support
- **Gas pricing strategies** — Implement dynamic gas pricing based on network conditions
- **Nonce management** — Customize how nonces are tracked and reset
- **Transaction submission** — Add retry logic, batching, or fallback to different RPC endpoints

To do this:

1. Create a new type that wraps or replaces `Eip155ChainProvider`
2. Implement `Eip155MetaTransactionProvider` for your type
3. Implement `ChainProviderOps` and `FromConfig` for your type
4. Use your custom provider when building the `ChainRegistry`

### Chain-Specific Facilitator Deployment

If you want to run a facilitator that only supports specific chains (e.g., only Solana, only EVM chains), you have two options:

**Option 1: Use the `facilitator` crate with feature flags**

The `facilitator` crate supports feature flags to enable only specific chains. Since this crate is not published on crates.io, use a git dependency:

```toml
[dependencies]
x402-facilitator = { git = "https://github.com/x402-rs/x402-rs", default-features = false, features = ["chain-solana"] }
```

Available features:
- `chain-eip155` — Enable EIP-155 (EVM) chain support
- `chain-solana` — Enable Solana chain support
- `chain-aptos` — Enable Aptos chain support
- `telemetry` — Enable OpenTelemetry tracing

Then in your `main.rs`, simply call the `run` function:

```rust
#[tokio::main]
async fn main() {
    let result = x402_facilitator::run().await;
    if let Err(e) = result {
        eprintln!("{e}");
        std::process::exit(1)
    }
}
```

**Option 2: Create a minimal custom facilitator**

Follow the "Getting Started" section above, but only register the schemes you need:

```rust
// Only register Solana schemes
let scheme_blueprints = {
    let mut blueprints = SchemeBlueprints::new();
    blueprints.register(V1SolanaExact);
    blueprints.register(V2SolanaExact);
    blueprints
};
```

This approach gives you full control over the binary size and dependencies.

### Middleware Integration

Integrate with your existing HTTP framework:

```rust
use axum::{middleware, Router};

let app = Router::new()
    .route("/verify", post(verify_handler))
    .route("/settle", post(settle_handler))
    .layer(middleware::from_fn(your_auth_middleware));
```

### Adding Pre/Post Processing Logic

You can wrap `FacilitatorLocal` to add custom logic before or after payment verification and settlement. This is useful for:

- **Logging and auditing** — Log all payment attempts for compliance
- **Rate limiting** — Enforce limits on verification/settlement calls
- **Custom validation** — Add business-specific validation rules
- **Metrics collection** — Track payment success rates, latency, etc.

To do this, create a wrapper struct and implement the `Facilitator` trait:

```rust
use x402_facilitator_local::FacilitatorLocal;
use x402_types::facilitator::Facilitator;
use x402_types::proto;
use std::sync::Arc;

/// A wrapper around FacilitatorLocal that adds custom pre/post processing.
pub struct FancyFacilitator<A> {
    inner: FacilitatorLocal<A>,
}

impl<A> FancyFacilitator<A> {
    pub fn new(inner: FacilitatorLocal<A>) -> Self {
        Self { inner }
    }
}

impl<A: Clone + Send + Sync + 'static> Facilitator for FancyFacilitator<A>
where
    FacilitatorLocal<A>: Facilitator,
{
    type Error = <FacilitatorLocal<A> as Facilitator>::Error;

    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, Self::Error> {
        // Pre-processing: custom validation, logging, rate limiting, etc.
        println!("Verifying payment for scheme: {:?}", request.scheme);
        
        // Delegate to inner facilitator
        let response = self.inner.verify(request).await?;
        
        // Post-processing: audit logging, metrics, etc.
        println!("Payment verified: payer={}", response.payer);
        
        Ok(response)
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, Self::Error> {
        // Pre-processing
        println!("Settling payment...");
        
        // Delegate to inner facilitator
        let response = self.inner.settle(request).await?;
        
        // Post-processing
        println!("Payment settled: tx={}", response.transaction);
        
        Ok(response)
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, Self::Error> {
        self.inner.supported().await
    }
}

// Usage:
let facilitator = FacilitatorLocal::new(scheme_registry);
let fancy_facilitator = FancyFacilitator::new(facilitator);
let state = Arc::new(fancy_facilitator);
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
