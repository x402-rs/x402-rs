# x402-reqwest-upto-eip155

An example client that uses [`x402-reqwest`](https://crates.io/crates/x402-reqwest) to pay for HTTP requests using the EIP-155 V2 **upto** x402 payment scheme.

The `upto` scheme authorizes a maximum token amount with Permit2. The server can settle for any amount up to that maximum after it measures actual usage, which is useful for metered endpoints.

## What it does

On startup, this example:
- Reads `EVM_PRIVATE_KEY` from the environment or `.env`
- Registers `V2Eip155UptoClient` with the x402 reqwest middleware
- Sends a request to an x402-protected endpoint, defaulting to `http://localhost:3000/eip155-upto`

If the server responds with `402 Payment Required`, the middleware:
- Parses the V2 `Payment-Required` header
- Selects a supported EIP-155 `upto` payment requirement
- Signs a Permit2 authorization for the maximum amount
- Retries the request with the `Payment-Signature` header attached

## Prerequisites

- An EVM private key with testnet funds and the token required by the endpoint
- Existing Permit2 allowance for the token and x402 upto Permit2 proxy
- An EVM RPC URL for reading EIP-2612 token nonces (`EVM_RPC_URL`, defaults to Base mainnet)
- Rust + Cargo
- `EVM_PRIVATE_KEY` set in your environment or in `.env`

## Running the Example

```shell
# 1. Clone the repo and cd into this example folder
# 2. Create `.env`
cp .env.example .env
# 3. Set EVM_PRIVATE_KEY inside `.env`
# 4. Run
cargo run
```

To call a different endpoint:

```shell
ENDPOINT=https://example.com/metered cargo run
```

## Related

- [`x402-reqwest`](https://crates.io/crates/x402-reqwest): Reqwest middleware for paying x402 requests
- [`x402-chain-eip155`](https://crates.io/crates/x402-chain-eip155): EIP-155 exact and upto scheme support
- [`x402-axum`](https://crates.io/crates/x402-axum): Axum server-side middleware to accept payments
