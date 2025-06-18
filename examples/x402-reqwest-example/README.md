# x402-reqwest-example

An example client that uses [`x402-reqwest`](https://crates.io/crates/x402-reqwest) to pay for HTTP requests using the x402 protocol.

This small demo shows how to configure a reqwest client to:
- Interact with x402-protected endpoints
- Apply token preferences and per-token payment caps

## What it does

On startup, this example:
- Reads your private key from env variable `PRIVATE_KEY`
- Builds a `reqwest` client using [`reqwest-middleware`](https://crates.io/crates/reqwest-middleware) and  [`x402-reqwest`](https://crates.io/crates/x402-reqwest)
- Sends a request to a protected endpoint

If the server responds with a 402 Payment Required, the client:
-	Parses the server’s requirements
-	Selects a supported token (e.g. USDC on Base Sepolia)
-	Signs a `TransferWithAuthorization`
-	Retries with the signed payment attached

The best part? **You don’t have to worry about any of this.**
Just set your token preferences and treat it like any other `reqwest` HTTP client.

# Prerequisites
- A private key with testnet funds (Base Sepolia USDC)
-	Rust + Cargo
-	`PRIVATE_KEY` set in your environment (or in `.env` file, see `.env.example`)

## Running the Example
```shell
# 1. Clone the repo and cd into this example folder
# 2. Create `.env` file
cp .env.example .env
# 3. Set your PRIVATE_KEY inside `.env`
# 4. Run
cargo run
```
You should see the request succeed and print the server’s response.

## Behind the scenes

This example uses:
-	[`x402-reqwest`](https://crates.io/crates/x402-reqwest) to intercept 402s and attach signed payments
-	[`alloy`](https://alloy.rs) for signing
-	[`dotenvy`](https://crates.io/crates/dotenvy) to load the `.env` file
-	[`x402-rs`](https://crates.io/crates/x402-rs) for token/network definitions and amount conversion

## Related
- [`x402-rs`](https://crates.io/crates/x402-rs): Common types and facilitator logic
- [`x402-axum`](https://crates.io/crates/x402-axum): Axum server-side middleware to accept payments
- [`x402-reqwest`](https://crates.io/crates/x402-reqwest): The crate this example is showcasing
