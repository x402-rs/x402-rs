# x402-reqwest

[![Crates.io](https://img.shields.io/crates/v/x402-reqwest.svg)](https://crates.io/crates/x402-reqwest)
[![Docs.rs](https://docs.rs/x402-reqwest/badge.svg)](https://docs.rs/x402-reqwest)

**Wrapper around [`reqwest`](https://crates.io/crates/reqwest) that transparently handles HTTP `402 Payment Required` responses using the [x402 protocol](https://x402.org/).**

This crate enables your [`reqwest`](https://crates.io/crates/reqwest) or [`reqwest-middleware`](https://crates.io/crates/reqwest-middleware)-based HTTP clients to:
- Detect `402 Payment Required` responses
- Build and sign [x402](https://x402.org) payment payloads
- Retry the request with the `X-Payment` header attached
- Respect client-defined preferences like token priority and per-token payment caps

All in all: **automatically pay for resources using the x402 protocol**.

Built with [`reqwest-middleware`](https://crates.io/crates/reqwest-middleware) and compatible with any [`alloy::Signer`](https://alloy.rs).

## Features

- Pluggable [`reqwest`](https://crates.io/crates/reqwest) middleware
- EIP-712-compatible signing with [`alloy`](https://alloy.rs)
- Fluent builder-style configuration
- Token preferences & per-asset payment limits
- Tracing support (opt-in via `telemetry` feature)

## Installation

Add the dependency:

```toml
# Cargo.toml
x402-reqwest = "0.1"
```

To enable tracing:

```toml
x402-reqwest = { version = "0.1", features = ["telemetry"] }
```

## 💡 Examples

```rust
use reqwest::Client;
use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, MaxTokenAmountFromAmount};
use alloy::signers::local::PrivateKeySigner;
use x402_rs::network::{Network, USDCDeployment};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let signer: PrivateKeySigner = "0x...".parse()?; // never hardcode real keys!

    let client = Client::new()
        .with_payments(signer)
        .prefer(USDCDeployment::by_network(Network::Base))
        .max(USDCDeployment::by_network(Network::Base).amount("1.00")?)
        .build();

    let res = client
        .get("https://example.com/protected")
        .send()
        .await?;

    println!("Status: {}", res.status());
    Ok(())
}
```

See more examples on [docs.rs](https://docs.rs/x402-reqwest)

## How it works
1.	A 402 Payment Required is received from a server.
2.	The middleware parses the Payment-Required response body.
3.	A compatible payment requirement is selected, based on client preferences.
4.	A signed payload is created (compatible with [EIP-3009](https://eips.ethereum.org/EIPS/eip-3009) `TransferWithAuthorization`).
5.	The payload is base64-encoded into an `X-Payment` header.
6.	The request is retried, now with the payment inside the header.

## Optional Features
- `telemetry`: Enables tracing annotations for richer observability.

Enable it via:
```toml
x402-reqwest = { version = "0.1", features = ["telemetry"] }
```

## Related Crates
- [x402-rs](https://crates.io/crates/x402-rs): Core x402 types, facilitator traits, helpers.
- [x402-axum](https://crates.io/crates/x402-rs): Axum middleware for accepting x402 payments.

## License

[Apache-2.0](LICENSE)
