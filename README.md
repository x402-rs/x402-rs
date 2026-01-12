# x402-rs

[![Crates.io](https://img.shields.io/crates/v/x402-rs.svg)](https://crates.io/crates/x402-rs)
[![Docs.rs](https://docs.rs/x402-rs/badge.svg)](https://docs.rs/x402-rs)
[![GHCR](https://img.shields.io/badge/ghcr.io-x402--facilitator-blue)](https://github.com/orgs/x402-rs/packages/container/package/x402-facilitator)

> A Rust-based implementation of the x402 protocol with support for protocol v1 and v2.

<div align="center">
<table><tr><td>
🚀 <strong>x402 Protocol v2</strong> — This is the main branch with x402 protocol v2 support, featuring a ground-up rethinking of how we accommodate various chains and payment schemes. The core facilitator is fully functional and ready for use. The <code>x402-axum</code> and <code>x402-reqwest</code> crates are being updated to leverage the new multi-chain, multi-scheme architecture and will be available soon. For protocol v1, see the <code>protocol-x402-v1</code> branch.
</td></tr></table>
</div>

This repository provides:

- `x402-rs` (current crate):
  - Core protocol types, facilitator traits, and logic for on-chain payment verification and settlement
  - Facilitator binary - production-grade HTTP server to verify and settle x402 payments
- [`x402-axum`](./crates/x402-axum) - Axum middleware for accepting x402 payments,
- [`x402-reqwest`](./crates/x402-reqwest) - Wrapper for reqwest for transparent x402 payments,
- [`x402-axum-example`](./examples/x402-axum-example) - an example of `x402-axum` usage with multi-chain support.
- [`x402-reqwest-example`](./examples/x402-reqwest-example) - an example of `x402-reqwest` usage with multi-chain support.

## About x402

The [x402 protocol](https://docs.cdp.coinbase.com/x402/docs/overview) is a proposed standard for making blockchain payments directly through HTTP using native `402 Payment Required` status code.

Servers declare payment requirements for specific routes. Clients send cryptographically signed payment payloads. Facilitators verify and settle payments on-chain.

## Getting Started

### Run facilitator

```shell
docker run -v $(pwd)/config.json:/app/config.json -p 8080:8080 ghcr.io/x402-rs/x402-facilitator
```

Or build locally:
```shell
docker build -t x402-rs .
docker run -v $(pwd)/config.json:/app/config.json -p 8080:8080 x402-rs
```

See the [Facilitator](#facilitator) section below for full usage details

### Protect Axum Routes

Use `x402-axum` to gate your routes behind on-chain payments:

```rust
use alloy_primitives::address;
use axum::{Router, routing::get};
use x402_axum::X402Middleware;
use x402_rs::networks::USDC;
use x402_rs::scheme::v2_eip155_exact::V2Eip155Exact;

let x402 = X402Middleware::try_from("https://facilitator.x402.rs").unwrap();

let app = Router::new().route(
    "/paid-content",
    get(handler).layer(
        x402.with_price_tag(V2Eip155Exact::price_tag(
            address!("0xYourAddress"),
            USDC::base_sepolia().amount(10u64),
        ))
    ),
);
```

See [`x402-axum` crate docs](./crates/x402-axum/README.md).

### Send x402 payments

Use `x402-reqwest` to send payments:

```rust
use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, X402Client};
use x402_rs::scheme::v1_eip155_exact::client::V1Eip155ExactClient;
use x402_rs::scheme::v2_eip155_exact::client::V2Eip155ExactClient;
use alloy_signer_local::PrivateKeySigner;
use std::sync::Arc;
use reqwest::Client;

let signer: Arc<PrivateKeySigner> = Arc::new("0x...".parse()?); // never hardcode real keys!

let x402_client = X402Client::new()
    .register(V1Eip155ExactClient::new(signer.clone()))
    .register(V2Eip155ExactClient::new(signer));

let client = Client::new()
    .with_payments(x402_client)
    .build();

let res = client
    .get("https://example.com/protected")
    .send()
    .await?;
```

The middleware automatically:
- Detects `402 Payment Required` responses
- Extracts payment requirements from the response
- Signs payments using registered scheme clients
- Retries the request with the payment header attached

For multi-chain support (EVM and Solana), register additional scheme clients:

```rust
use x402_rs::scheme::v1_solana_exact::client::V1SolanaExactClient;
use x402_rs::scheme::v2_solana_exact::client::V2SolanaExactClient;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_keypair::Keypair;

let solana_keypair = Arc::new(Keypair::from_base58_string("..."));
let solana_rpc = Arc::new(RpcClient::new("https://api.devnet.solana.com"));

let x402_client = X402Client::new()
    .register(V1Eip155ExactClient::new(evm_signer.clone()))
    .register(V2Eip155ExactClient::new(evm_signer))
    .register(V1SolanaExactClient::new(solana_keypair.clone(), solana_rpc.clone()))
    .register(V2SolanaExactClient::new(solana_keypair, solana_rpc));
```

See [`x402-reqwest` crate docs](./crates/x402-reqwest/README.md).

## Roadmap

| Milestone                           | Description                                                                                              |   Status   |
|:------------------------------------|:---------------------------------------------------------------------------------------------------------|:----------:|
| Facilitator for Base USDC           | Payment verification and settlement service, enabling real-time pay-per-use transactions for Base chain. | ✅ Complete |
| Metrics and Tracing                 | Expose OpenTelemetry metrics and structured tracing for observability, monitoring, and debugging         | ✅ Complete |
| Server Middleware                   | Provide ready-to-use integration for Rust web frameworks such as axum and tower.                         | ✅ Complete |
| Client Library                      | Provide a lightweight Rust library for initiating and managing x402 payment flows from Rust clients.     | ✅ Complete |
| Solana Support                      | Support Solana chain.                                                                                    | ✅ Complete |
| Protocol v2 Support                 | Support x402 protocol version 2 with improved payload structure.                                         | ✅ Complete |
| Multiple chains and multiple tokens | Support various tokens and EVM compatible chains.                                                        | ✅ Complete |
| Axum Middleware v2 Support          | Full x402 protocol v2 support in x402-axum with multi-chain, multi-scheme architecture.                  | ✅ Complete |
| Reqwest Client v2 Support           | Full x402 protocol v2 support in x402-reqwest with multi-chain, multi-scheme architecture.               | ✅ Complete |
| Buiild your own facilitator hooks   | Pre/post hooks for analytics, access control, and auditability.                                          | 🔜 Planned |
| Bazaar Extension                    | Marketplace integration for discovering and purchasing x402-protected resources.                         | 🔜 Planned |
| Gasless Approval Flow               | Support for Permit2 and ERC20 approvals to enable gasless payment authorization.                         | 🔜 Planned |
| Upto Scheme                         | Payment scheme supporting "up to" amount payments with flexible pricing.                                 | 🔜 Planned |
| Deferred Scheme                     | Payment scheme supporting deferred settlement and payment scheduling.                                    | 🔜 Planned |

The initial focus is on establishing a stable, production-quality Rust SDK and middleware ecosystem for x402 integration.

## Facilitator

The `x402-rs` crate (this repo) provides a runnable x402 facilitator binary. The _Facilitator_ role simplifies adoption of x402 by handling:
- **Payment verification**: Confirming that client-submitted payment payloads match the declared requirements.
- **Payment settlement**: Submitting validated payments to the blockchain and monitoring their confirmation.

By using a Facilitator, servers (sellers) do not need to:
- Connect directly to a blockchain.
- Implement complex cryptographic or blockchain-specific payment logic.

Instead, they can rely on the Facilitator to perform verification and settlement, reducing operational overhead and accelerating x402 adoption.
The Facilitator **never holds user funds**. It acts solely as a stateless verification and execution layer for signed payment payloads.

For a detailed overview of the x402 payment flow and Facilitator role, see the [x402 protocol documentation](https://docs.cdp.coinbase.com/x402/docs/overview).

### Usage

#### 1. Create a configuration file

Create a `config.json` file with your chain and scheme configuration:

```json
{
  "port": 8080,
  "host": "0.0.0.0",
  "chains": {
    "eip155:84532": {
      "eip1559": true,
      "flashblocks": true,
      "signers": ["$EVM_PRIVATE_KEY"],
      "rpc": [
        {
          "http": "https://sepolia.base.org",
          "rate_limit": 50
        }
      ]
    },
    "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG": {
      "signer": "$SOLANA_PRIVATE_KEY",
      "rpc": "https://api.devnet.solana.com",
      "pubsub": "wss://api.devnet.solana.com"
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

**Configuration structure:**

- **`chains`**: A map of CAIP-2 chain identifiers to chain-specific configuration
  - EVM chains (`eip155:*`): Configure `signers` (array of private keys), `rpc` endpoints, and optional `eip1559`/`flashblocks` flags
  - Solana chains (`solana:*`): Configure `signer` (single private key), `rpc` endpoint, and optional `pubsub` endpoint
- **`schemes`**: List of payment schemes to enable
  - `id`: Scheme identifier in format `v{version}-{namespace}-{name}` (e.g., `v2-eip155-exact`)
  - `chains`: Chain pattern to match (e.g., `eip155:*` for all EVM chains, `eip155:84532` for specific chain)

**Environment variable references:**

Private keys can reference environment variables using `$VAR` or `${VAR}` syntax:
```json
"signers": ["$EVM_PRIVATE_KEY"]
```

Then set the environment variable:
```shell
export EVM_PRIVATE_KEY=0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef
```

#### 2. Build and Run with Docker

Prebuilt Docker images are available at [GitHub Container Registry](https://github.com/orgs/x402-rs/packages/container/package/x402-facilitator): `ghcr.io/x402-rs/x402-facilitator`

Run the container:
```shell
docker run -v $(pwd)/config.json:/app/config.json -p 8080:8080 ghcr.io/x402-rs/x402-facilitator
```

Or build a Docker image locally:
```shell
docker build -t x402-rs .
docker run -v $(pwd)/config.json:/app/config.json -p 8080:8080 x402-rs
```

You can also pass environment variables for private keys:
```shell
docker run -v $(pwd)/config.json:/app/config.json \
  -e EVM_PRIVATE_KEY=0x... \
  -e SOLANA_PRIVATE_KEY=... \
  -p 8080:8080 ghcr.io/x402-rs/x402-facilitator
```

The container:
* Exposes port `8080` (or a port you configure in `config.json`).
* Starts on http://localhost:8080 by default.
* Requires minimal runtime dependencies (based on `debian:bullseye-slim`).

#### 3. Point your application to your Facilitator

If you are building an x402-powered application, update the Facilitator URL to point to your self-hosted instance.

> ℹ️ **Tip:** For production deployments, ensure your Facilitator is reachable via HTTPS and protect it against public abuse.

<details>
<summary>If you use Hono and x402-hono</summary>
From [x402.org Quickstart for Sellers](https://x402.gitbook.io/x402/getting-started/quickstart-for-sellers):

```typescript
import { Hono } from "hono";
import { serve } from "@hono/node-server";
import { paymentMiddleware } from "@x402/hono";
import { x402ResourceServer, HTTPFacilitatorClient } from "@x402/core/server";
import { registerExactEvmScheme } from "@x402/evm/exact/server";

const app = new Hono();
const payTo = "0xYourAddress";

const facilitatorClient = new HTTPFacilitatorClient({
  url: "https://x402.org/facilitator"
});

const server = new x402ResourceServer(facilitatorClient);
registerExactEvmScheme(server);

app.use(
  paymentMiddleware(
    {
      "/protected-route": {
        accepts: [
          {
            scheme: "exact",
            price: "$0.10",
            network: "eip155:84532",
            payTo,
          },
        ],
        description: "Access to premium content",
        mimeType: "application/json",
      },
    },
    server,
  ),
);

app.get("/protected-route", (c) => {
  return c.json({ message: "This content is behind a paywall" });
});

serve({ fetch: app.fetch, port: 3000 });
```

</details>

<details>
<summary>If you use `x402-axum`</summary>

```rust
use alloy_primitives::address;
use axum::{Router, routing::get};
use axum::response::IntoResponse;
use http::StatusCode;
use x402_axum::X402Middleware;
use x402_rs::networks::USDC;
use x402_rs::scheme::v2_eip155_exact::V2Eip155Exact;

let x402 = X402Middleware::new("https://facilitator.x402.rs");  // 👈 Your self-hosted Facilitator

let app = Router::new().route(
    "/paid-content",
    get(handler).layer(
        x402.with_price_tag(V2Eip155Exact::price_tag(
            address!("0xYourAddress"),
            USDC::base_sepolia().amount(10u64),
        ))
    ),
);

async fn handler() -> impl IntoResponse {
    (StatusCode::OK, "This is VIP content!")
}
```

</details>

### Configuration

The service reads configuration from a JSON file (`config.json` by default) or via CLI argument `--config <path>`.

#### Configuration File Structure

```json
{
  "port": 8080,
  "host": "0.0.0.0",
  "chains": { ... },
  "schemes": [ ... ]
}
```

#### Top-level Options

| Option | Type | Default | Description |
|:-------|:-----|:--------|:------------|
| `port` | number | `8080` | HTTP server port (can also be set via `PORT` env var) |
| `host` | string | `"0.0.0.0"` | HTTP host to bind to (can also be set via `HOST` env var) |
| `chains` | object | `{}` | Map of CAIP-2 chain IDs to chain configuration |
| `schemes` | array | `[]` | List of payment schemes to enable |

#### EVM Chain Configuration (`eip155:*`)

```json
{
  "eip155:84532": {
    "eip1559": true,
    "flashblocks": true,
    "receipt_timeout_secs": 30,
    "signers": ["$EVM_PRIVATE_KEY"],
    "rpc": [
      {
        "http": "https://sepolia.base.org",
        "rate_limit": 50
      }
    ]
  }
}
```

| Option                 | Type    | Required | Default | Description                                                                          |
|:-----------------------|:--------|:---------|:--------|:-------------------------------------------------------------------------------------|
| `signers`              | array   | ✅        | -       | Array of private keys (hex format, 0x-prefixed) or env var references                |
| `rpc`                  | array   | ✅        | -       | Array of RPC endpoint configurations                                                 |
| `rpc[].http`           | string  | ✅        | -       | HTTP URL for the RPC endpoint                                                        |
| `rpc[].rate_limit`     | number  | ❌        | -       | Rate limit for requests per second                                                   |
| `eip1559`              | boolean | ❌        | `true`  | Use EIP-1559 transaction type (type 2) instead of legacy transactions                |
| `flashblocks`          | boolean | ❌        | `false` | Estimate gas against "latest" block to accommodate flashblocks-enabled RPC semantics |
| `receipt_timeout_secs` | number  | ❌        | `30`    | Timeout for waiting for transaction receipt                                          |

#### Solana Chain Configuration (`solana:*`)

```json
{
  "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp": {
    "signer": "$SOLANA_PRIVATE_KEY",
    "rpc": "https://api.mainnet-beta.solana.com",
    "pubsub": "wss://api.mainnet-beta.solana.com",
    "max_compute_unit_limit": 400000,
    "max_compute_unit_price": 1000000
  }
}
```

| Option                   | Type   | Required | Default   | Description                                                |
|:-------------------------|:-------|:---------|:----------|:-----------------------------------------------------------|
| `signer`                 | string | ✅        | -         | Private key (base58 format, 64 bytes) or env var reference |
| `rpc`                    | string | ✅        | -         | HTTP URL for the RPC endpoint                              |
| `pubsub`                 | string | ❌        | -         | WebSocket URL for pubsub notifications                     |
| `max_compute_unit_limit` | number | ❌        | `400000`  | Maximum compute unit limit for transactions                |
| `max_compute_unit_price` | number | ❌        | `1000000` | Maximum compute unit price for transactions                |

#### Scheme Configuration

```json
{
  "schemes": [
    {
      "enabled": true,
      "id": "v2-eip155-exact",
      "chains": "eip155:*",
      "config": {}
    }
  ]
}
```

| Option | Type | Required | Default | Description |
|:-------|:-----|:---------|:--------|:------------|
| `enabled` | boolean | ❌ | `true` | Whether this scheme is enabled |
| `id` | string | ✅ | - | Scheme identifier: `v{version}-{namespace}-{name}` |
| `chains` | string | ✅ | - | Chain pattern: `eip155:*`, `solana:*`, or specific chain ID |
| `config` | object | ❌ | - | Scheme-specific configuration |

**Important:** Schemes must be explicitly listed in the `schemes` array to be enabled. If a scheme is not in the configuration, it will not be available for payment verification or settlement.

**Available schemes:**
- `v1-eip155-exact` - ERC-3009 transferWithAuthorization for EVM chains (protocol v1)
- `v2-eip155-exact` - ERC-3009 transferWithAuthorization for EVM chains (protocol v2)
- `v1-solana-exact` - SPL token transfer for Solana (protocol v1)
- `v2-solana-exact` - SPL token transfer for Solana (protocol v2)

#### Environment Variables

Environment variables can be used for:
- **Private keys**: Reference in config with `$VAR` or `${VAR}` syntax
- **Server settings**: `PORT` and `HOST` as fallbacks if not in config file
- **Logging**: `RUST_LOG` for log level (e.g., `info`, `debug`, `trace`)

### Observability

The facilitator emits [OpenTelemetry](https://opentelemetry.io)-compatible traces and metrics to standard endpoints,
making it easy to integrate with tools like Honeycomb, Prometheus, Grafana, and others.
Tracing spans are annotated with HTTP method, status code, URI, latency, other request and process metadata.

To enable tracing and metrics export, set the appropriate `OTEL_` environment variables:

```dotenv
# For Honeycomb, for example:
# Endpoint URL for sending OpenTelemetry traces and metrics
OTEL_EXPORTER_OTLP_ENDPOINT=https://api.honeycomb.io:443
# Comma-separated list of key=value pairs to add as headers
OTEL_EXPORTER_OTLP_HEADERS=x-honeycomb-team=your_api_key,x-honeycomb-dataset=x402-rs
# Export protocol to use for telemetry. Supported values: `http/protobuf` (default), `grpc`
OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf
```

The service automatically detects and initializes exporters if `OTEL_EXPORTER_OTLP_*` variables are provided.

### Supported Networks

The Facilitator supports any network you configure in `config.json`. Common chain identifiers:

| Network                   | CAIP-2 Chain ID                                       | Notes                            |
|:--------------------------|:------------------------------------------------------|:---------------------------------|
| Base Sepolia Testnet      | `eip155:84532`                                        | Testnet, Recommended for testing |
| Base Mainnet              | `eip155:8453`                                         | Mainnet                          |
| Ethereum Mainnet          | `eip155:1`                                            | Mainnet                          |
| Avalanche Fuji Testnet    | `eip155:43113`                                        | Testnet                          |
| Avalanche C-Chain Mainnet | `eip155:43114`                                        | Mainnet                          |
| Polygon Amoy Testnet      | `eip155:80002`                                        | Testnet                          |
| Polygon Mainnet           | `eip155:137`                                          | Mainnet                          |
| Sei Testnet               | `eip155:713715`                                       | Testnet                          |
| Sei Mainnet               | `eip155:1329`                                         | Mainnet                          |
| Solana Mainnet            | `solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp`             | Mainnet                          |
| Solana Devnet             | `solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1wcaWoxPkrZBG` | Testnet, Recommended for testing |

Networks are enabled by adding them to the `chains` section in your `config.json`.

> ℹ️ **Tip:** For initial development and testing, you can start with Base Sepolia (`eip155:84532`) or Solana Devnet only.

### Development

Prerequisites:
- Rust 1.80+
- `cargo` and a working toolchain

Build locally:
```shell
cargo build
```

Run with a config file:
```shell
cargo run -- --config config.json
```

Or place `config.json` in the current directory (it will be auto-detected):
```shell
cargo run
```

## Related Resources

* [x402 Protocol Documentation](https://x402.org)
* [x402 Overview by Coinbase](https://docs.cdp.coinbase.com/x402/docs/overview)
* [Facilitator Documentation by Coinbase](https://docs.cdp.coinbase.com/x402/docs/facilitator)

## Contributions and feedback welcome!
Feel free to open issues or pull requests to improve x402 support in the Rust ecosystem.

## License

[Apache-2.0](LICENSE)
