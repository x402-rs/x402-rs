# x402-rs

[![Crates.io](https://img.shields.io/crates/v/x402-rs.svg)](https://crates.io/crates/x402-rs)
[![Docs.rs](https://docs.rs/x402-rs/badge.svg)](https://docs.rs/x402-rs)
[![Docker Pulls](https://img.shields.io/docker/pulls/ukstv/x402-facilitator.svg)](https://hub.docker.com/r/ukstv/x402-facilitator)
[![GHCR](https://img.shields.io/badge/ghcr.io-x402--facilitator-blue)](https://github.com/orgs/x402-rs/packages)

> Rust-based implementation of the x402 protocol facilitator.

[x402 Protocol](https://docs.cdp.coinbase.com/x402/docs/overview) defines a standard for making blockchain payments directly through HTTP 402 flows.

The _Facilitator_ simplifies adoption of x402 by handling:

* **Payment verification**: Confirming that client-submitted payment payloads match the declared requirements.
* **Payment settlement**: Submitting validated payments to the blockchain and monitoring their confirmation.

By using a Facilitator, servers (sellers) do not need to:

* Connect directly to a blockchain.
* Implement complex cryptographic or blockchain-specific payment logic.

Instead, they can rely on the Facilitator to perform verification and settlement, reducing operational overhead and accelerating x402 adoption.

## Current Features

This repository currently implements a _Facilitator_ role according to the x402 specification.

**Responsibilities:**
- _Verify Payments:_ Confirm that submitted payment payloads are valid.
- _Settle Payments:_ Submit validated payments on-chain.
- _Return Results:_ Provide clear verification and settlement responses to the resource server.
- _Emit Traces:_ Emit OpenTelemetry-compatible traces and metrics.

The Facilitator **does not hold funds**. It acts purely as an execution and verification layer based on signed payloads.

For a detailed overview of the x402 payment flow and Facilitator role, see the [x402 protocol documentation](https://docs.cdp.coinbase.com/x402/docs/overview).

## Roadmap

This project provides a Rust-based implementation of the x402 payment protocol, focused on reliability, ease of integration, and support for machine-native payment flows.

| Milestone                           | Description                                                                                              |   Status   |
|:------------------------------------|:---------------------------------------------------------------------------------------------------------|:----------:|
| Facilitator for Base USDC           | Payment verification and settlement service, enabling real-time pay-per-use transactions for Base chain. | ✅ Complete |
| Metrics and Tracing                 | Expose OpenTelemetry metrics and structured tracing for observability, monitoring, and debugging         | ✅ Complete |
| Server Middleware                   | Provide ready-to-use integration for Rust web frameworks such as axum and tower.                         | 🔜 Planned |
| Client Library                      | Provide a lightweight Rust library for initiating and managing x402 payment flows from Rust clients.     | 🔜 Planned |
| Multiple chains and multiple tokens | Support various tokens and EVM compatible chains.                                                        | 🔜 Planned |
| Payments Storage                    | Persist verified and settled payments for analytics, access control, and auditability.                   | 🔜 Planned |
| Micropayments                       | Enable fine-grained offchain usage-based payments, including streaming and per-request billing.          | 🔜 Planned |

---

The initial focus is on establishing a stable, production-quality Rust SDK and middleware ecosystem for x402 integration.

## Usage

### 1. Provide environment variables

Create a `.env` file or set environment variables directly. Example `.env`:

```dotenv
HOST=0.0.0.0
PORT=8080
RPC_URL_BASE_SEPOLIA=https://sepolia.base.org
RPC_URL_BASE=https://mainnet.base.org
SIGNER_TYPE=private-key
PRIVATE_KEY=0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef
RUST_LOG=info
```

**Important:**
The supported networks are determined by which RPC URLs you provide:
- If you set only `RPC_URL_BASE_SEPOLIA`, then only Base Sepolia network is supported.
- If you set both `RPC_URL_BASE_SEPOLIA` and `RPC_URL_BASE`, then both Base Sepolia and Base Mainnet are supported.
- If an RPC URL for a network is missing, that network will not be available for settlement or verification.

### 2. Build and Run with Docker

Prebuilt Docker images are available at:
- [GitHub Container Registry](https://ghcr.io/x402-rs/x402-facilitator): `ghcr.io/x402-rs/x402-facilitator`
- [Docker Hub](https://hub.docker.com/r/ukstv/x402-facilitator): `ukstv/x402-facilitator`

Run the container from Docker Hub:
```commandline
docker run --env-file .env -p 8080:8080 ukstv/x402-facilitator
```

To run using GitHub Container Registry:
```commandline
docker run --env-file .env -p 8080:8080 ghcr.io/x402-rs/x402-facilitator
```

Or build a Docker image locally:
```commandline
docker build -t x402-rs .
docker run --env-file .env -p 8080:8080 x402-rs
```

The container:
* Exposes port `8080` (or a port you configure with `PORT` environment variable).
* Starts on http://localhost:8080 by default.
* Requires minimal runtime dependencies (based on `debian:bullseye-slim`).

### 3. Point your application to your Facilitator

If you are building an x402-powered application, update the Facilitator URL to point to your self-hosted instance.

Example using Hono and x402-hono (from [x402.org Quickstart for Sellers](https://x402.gitbook.io/x402/getting-started/quickstart-for-sellers)):

```typescript
import { Hono } from "hono";
import { serve } from "@hono/node-server";
import { paymentMiddleware } from "x402-hono";

const app = new Hono();

// Configure the payment middleware
app.use(paymentMiddleware(
  "0xYourAddress", // Your receiving wallet address
  {
    "/protected-route": {
      price: "$0.10",
      network: "base-sepolia",
      config: {
        description: "Access to premium content",
      }
    }
  },
  {
    url: "http://localhost:8080", // 👈 Your self-hosted Facilitator
  }
));

// Implement your protected route
app.get("/protected-route", (c) => {
  return c.json({ message: "This content is behind a paywall" });
});

serve({
  fetch: app.fetch,
  port: 3000
});
```

> ℹ️ **Tip:** For production deployments, ensure your Facilitator is reachable via HTTPS and protect it against public abuse.

## Configuration

The service reads configuration via `.env` file or directly through environment variables.

Available variables:

* `SIGNER_TYPE` (required): Type of signer to use. Only `private-key` is supported now.
* `PRIVATE_KEY` (required): Private key in hex, like `0xdeadbeef...`.
* `HOST`: HTTP host to bind to (default `0.0.0.0`)
* `PORT`: HTTP server port (default `8080`)
* `RUST_LOG`: Logging level (e.g., `info`, `debug`, `trace`)
* `RPC_URL_BASE_SEPOLIA`: Ethereum RPC endpoint for Base Sepolia testnet
* `RPC_URL_BASE`: Ethereum RPC endpoint for Base mainnet

### Observability

The facilitator emits [OpenTelemetry](https://opentelemetry.io)-compatible traces and metrics to standard endpoints,
making it easy to integrate with tools like Honeycomb, Prometheus, Grafana, and others.
Spans are tagged with request method, status code, URI, and latency.
Metrics include basic process and request-level metrics

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

The Facilitator supports different networks based on the environment variables you configure:

| Network      | Environment Variable   | Supported if Set | Notes                   |
|:-------------|:-----------------------|:-----------------|:------------------------|
| Base Sepolia | `RPC_URL_BASE_SEPOLIA` | ✅                | Recommended for testing |
| Base Mainnet | `RPC_URL_BASE`         | ✅                | Mainnet deployment      |

- If you provide only `RPC_URL_BASE_SEPOLIA`, only **Base Sepolia** will be available.
- If you provide both `RPC_URL_BASE_SEPOLIA` and `RPC_URL_BASE`, then both networks will be supported.

> ℹ️ **Tip:** For initial development and testing, you can start with Base Sepolia only.

## Development

Prerequisites:
- Rust 1.80+
- `cargo` and a working toolchain

Build locally:
```bash
cargo build
```
Run:
```bash
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
