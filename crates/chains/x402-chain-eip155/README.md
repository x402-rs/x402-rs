# x402-chain-eip155

EIP-155 (EVM) chain support for the [x402](https://www.x402.org) payment protocol.

This crate provides implementations of the [x402](https://www.x402.org) payment protocol for EVM-compatible blockchains using the EIP-155 chain
ID standard. It supports both V1 and V2 protocol versions with the "exact" payment scheme based on ERC-3009
`transferWithAuthorization`.

## Features

- **V1 and V2 Protocol Support**: Implements both protocol versions with network name (V1) and CAIP-2 chain ID (V2)
  addressing
- **ERC-3009 Payments**: Gasless token transfers using `transferWithAuthorization`
- **Smart Wallet Support**:
  - EIP-1271 for deployed smart wallets
  - EIP-6492 for counterfactual (not-yet-deployed) smart wallets
  - EOA (Externally Owned Account) signatures
- **Multiple Signers**: Round-robin signer selection for load distribution
- **Nonce Management**: Automatic nonce tracking with pending transaction awareness
- **Gas Management**: Automatic gas estimation with EIP-1559 and legacy support

## Architecture

The crate is organized into several modules:

- **`chain`** - Core EVM chain types, providers, and configuration
- **`v1_eip155_exact`** - V1 protocol implementation with network names
- **`v2_eip155_exact`** - V2 protocol implementation with CAIP-2 chain IDs

## Feature Flags

- `server` - Server-side price tag generation
- `client` - Client-side payment signing
- `facilitator` - Facilitator-side payment verification and settlement
- `telemetry` - OpenTelemetry tracing support

## Usage

### Server: Creating a Price Tag

```rust
use x402_chain_eip155::{V1Eip155Exact, KnownNetworkEip155};
use x402_types::networks::USDC;

// Get USDC deployment on Base
let usdc = USDC::base();

// Create a price tag for 1 USDC
let price_tag = V1Eip155Exact::price_tag(
"0x742d35Cc6634C0532925a3b844Bc9e7595f0bEb",
usdc.amount(1_000_000u64),
);
```

### Client: Signing a Payment

```rust
use x402_chain_eip155::V1Eip155ExactClient;
use alloy_signer_local::PrivateKeySigner;

let signer = PrivateKeySigner::random();
let client = V1Eip155ExactClient::new(signer);

// Use client to sign payment candidates
let candidates = client.accept( & payment_required);
```

### Facilitator: Verifying and Settling

```rust
use x402_chain_eip155::{V1Eip155Exact, Eip155ChainProvider};
use x402_types::scheme::X402SchemeFacilitatorBuilder;

let provider = Eip155ChainProvider::from_config( & config).await?;
let facilitator = V1Eip155Exact.build(provider, None) ?;

// Verify payment
let verify_response = facilitator.verify( & verify_request).await?;

// Settle payment
let settle_response = facilitator.settle( & settle_request).await?;
```

## Supported Networks

The crate includes built-in support for many EVM networks through the `KnownNetworkEip155` trait:

- **Base** (mainnet and Sepolia testnet)
- **Polygon** (mainnet and Amoy testnet)
- **Avalanche** (C-Chain and Fuji testnet)
- **Sei** (mainnet and testnet)
- **XDC Network**
- **XRPL EVM**
- **Peaq**
- **IoTeX**
- **Celo** (mainnet and Sepolia testnet)

Each network includes USDC token deployment information with proper EIP-712 domain parameters.

## ERC-3009 and Signature Handling

The facilitator intelligently dispatches to different `transferWithAuthorization` contract functions or other onchain functions based on the
signature format:

- **EOA signatures (64-65 bytes)**: Parsed as (r, s, v) components and dispatched to the standard EIP-3009 function
- **EIP-1271 signatures**: Passed as full signature bytes for contract wallet verification
- **EIP-6492 signatures**: Detected by the 32-byte magic suffix and validated via the universal EIP-6492 validator
  contract

For EIP-6492 counterfactual signatures, the facilitator can deploy the smart wallet on-chain if needed before settling
the payment.

## Configuration

### Facilitator Configuration Example

```json
{
  "eip155:8453": {
    "eip1559": true,
    "flashblocks": false,
    "receipt_timeout_secs": 30,
    "signers": [
      "$FACILITATOR_PRIVATE_KEY"
    ],
    "rpc": [
      {
        "http": "https://mainnet.base.org",
        "rate_limit": 100
      }
    ]
  }
}
```

## Dependencies

This crate uses the [Alloy](https://github.com/alloy-rs/alloy) library for Ethereum interactions, providing:

- Type-safe contract bindings
- EIP-712 typed data signing
- Transaction building and signing
- RPC provider with fallback and rate limiting

## License

Apache 2.0
