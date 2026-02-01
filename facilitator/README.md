# x402-facilitator

A production-ready x402 facilitator server binary.

This crate provides a complete, runnable HTTP server that implements the [x402](https://www.x402.org) payment protocol. It supports multiple blockchain networks (EVM/EIP-155, Solana, Aptos) and can verify and settle payments on-chain.

The crate can also be used as a library to build custom facilitator implementations.

## Features

- **Multi-chain Support**: EVM (EIP-155), Solana, and Aptos blockchains
- **Multiple Payment Schemes**: V1 and V2 protocol implementations
- **OpenTelemetry Integration**: Optional distributed tracing and metrics (`telemetry` feature)
- **Graceful Shutdown**: Clean shutdown on SIGTERM/SIGINT signals
- **CORS Support**: Cross-origin requests enabled for web clients
- **Flexible Configuration**: JSON-based configuration with environment variable overrides
- **Modular Chain Support**: Enable only the blockchain networks you need via feature flags

## Installation

### As a Binary (via cargo install)

```bash
# Install from git
cargo install --git https://github.com/x402-rs/x402-rs --package x402-facilitator

# Run the installed binary
x402-facilitator --config /path/to/config.json # Or provide config path via $CONF env var
```

### As a Library

Add to your `Cargo.toml`:

```toml
[dependencies]
x402-facilitator = { git = "https://github.com/x402-rs/x402-rs" }
```

**Note**: If you enable the `chain-aptos` feature, you must also include the required patches in your `Cargo.toml`:

```toml
[patch.crates-io]
merlin = { git = "https://github.com/aptos-labs/merlin" }

[patch."https://github.com/aptos-labs/aptos-core"]
aptos-runtimes = { path = "https://github.com/x402-rs/x402-rs/patches/aptos-runtimes" }
```

## Usage

### Running the Server

```bash
# Build and run from source
cargo run --package x402-facilitator

# With telemetry
cargo run --package x402-facilitator --features telemetry

# With specific chains only
cargo run --package x402-facilitator --features chain-eip155,chain-solana

# With all chains including Aptos (requires patches)
cargo run --package x402-facilitator --features chain-eip155,chain-solana,chain-aptos

# With the full feature (all chains + telemetry)
cargo run --package x402-facilitator --features full

# Specify custom config file
cargo run --package x402-facilitator -- --config /path/to/config.json
```

### Configuration

Create a `config.json` file:

```json
{
  "port": 8080,
  "host": "0.0.0.0",
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

### Environment Variables

| Variable                      | Description                      | Default       |
|-------------------------------|----------------------------------|---------------|
| `HOST`                        | Server bind address              | `0.0.0.0`     |
| `PORT`                        | Server port                      | `8080`        |
| `CONFIG`                      | Path to config file              | `config.json` |
| `OTEL_EXPORTER_OTLP_ENDPOINT` | OpenTelemetry collector endpoint | -             |
| `OTEL_SERVICE_NAME`           | Service name for traces          | -             |

## HTTP Endpoints

| Endpoint     | Method | Description             |
|--------------|--------|-------------------------|
| `/`          | GET    | Server greeting         |
| `/verify`    | GET    | Schema information      |
| `/verify`    | POST   | Verify payment payload  |
| `/settle`    | GET    | Schema information      |
| `/settle`    | POST   | Settle payment on-chain |
| `/supported` | GET    | List supported schemes  |
| `/health`    | GET    | Health check            |

## Architecture

The facilitator is built on top of the `x402-facilitator-local` crate and uses:

- **Axum**: HTTP server framework
- **Tokio**: Async runtime
- **x402-types**: Core protocol types and configuration (via `x402_types::config`)
- **x402-chain-\\\\*\\\**: Chain-specific implementations

```text
┌─────────────┐
│   Axum HTTP │
│   Server    │
└──────┬──────┘
       │
┌──────▼──────┐
│ Facilitator │
│   Local     │
└──────┬──────┘
       │
┌──────▼──────┐
│   Scheme    │
│  Registry   │
└──────┬──────┘
       │
  ┌────┴────┐
  ▼         ▼
┌─────┐  ┌─────┐  ┌─────┐
│EIP  │  │Sol  │  │Apt  │
│155  │  │ana  │  │os   │
└─────┘  └─────┘  └─────┘
```

## Feature Flags

| Feature        | Description                                   |
|----------------|-----------------------------------------------|
| `telemetry`    | Enable OpenTelemetry tracing and metrics      |
| `chain-eip155` | Enable EVM/EIP-155 chain support              |
| `chain-solana` | Enable Solana chain support                   |
| `chain-aptos`  | Enable Aptos chain support (requires patches) |
| `full`         | Enable all features: telemetry + all chains   |

**Note**: The `chain-aptos` feature requires additional patches due to its dependencies on Aptos core libraries. See the [Installation](#installation) section for details.

## License

Apache-2.0
