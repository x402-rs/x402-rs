# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.4.2] - 2026-02-27

### Added

- `x402-chain-eip155`: Enabled the `traceparent` feature on `alloy-transport-http`, so outgoing EVM RPC calls now propagate W3C `traceparent` headers for distributed tracing.

### Changed

- `facilitator`: Refactored telemetry layer initialization — `Telemetry::register()` now returns the providers handle directly, and `http_tracing()` is called on it in a separate `#[cfg(feature = "telemetry")]` statement, making the initialization sequence clearer and easier to extend.

## [1.4.1] - 2026-02-25

### Changed

- `x402-types`: `LiteralOrEnv<T>` now stores the original environment variable name alongside the resolved value, so `Display` reconstructs the `$VAR_NAME` syntax instead of rendering the resolved value. This prevents sensitive values from being exposed in logs or serialized output and enables config round-tripping.

## [1.4.0] - 2026-02-25

### Added

- `x402-types`: Implemented `Display` for `LiteralOrEnv<T>`, allowing env-var-wrapped config values to be formatted directly.

### Changed

- `x402-chain-eip155`: `RpcConfig::http` field type changed from `Url` to `LiteralOrEnv<Url>`, enabling the RPC endpoint URL to be supplied via an environment variable reference in config files.
- `x402-chain-solana`: `SolanaChainConfigInner::rpc` and `pubsub` fields changed from `Url` / `Option<Url>` to `LiteralOrEnv<Url>` / `Option<LiteralOrEnv<Url>>`, enabling RPC and pubsub endpoint URLs to be supplied via environment variable references in config files.
- `x402-chain-solana`: `SolanaChainConfig::pubsub()` return type changed from `&Option<Url>` to `Option<&Url>` for a more idiomatic API.

## [1.1.0] - 2026-02-05

### Fixed

- Fixed a "value" serde bug that caused incorrect deserialization of payment values.

## [1.0.0] - 2025-02-02

### Changed

- **BREAKING**: Refactored from a single monolithic crate into a modular workspace architecture. The `x402-rs` crate is no longer published as a single package.
- **BREAKING**: Import paths have changed from `x402_rs::*` to `x402_types::*` and specific chain crates.

### Added

- New `x402-types` crate: Core protocol types, facilitator traits, and utilities.
- New `x402-chain-eip155` crate: EVM/EIP-155 chain support (Ethereum, Base, Polygon, etc.) with feature flags for `client`, `server`, and `facilitator`.
- New `x402-chain-solana` crate: Solana blockchain support with feature flags for `client`, `server`, and `facilitator`.
- New `x402-chain-aptos` crate: Aptos blockchain support with feature flags for `client`, `server`, and `facilitator`.
- New `x402-facilitator-local` crate: Local facilitator implementation for payment verification and settlement.
- New `facilitator` binary crate: Production-ready facilitator server (not published to crates.io).
- New documentation: `docs/build-your-own-facilitator.md` guide.
- Workspace-level dependency management in root `Cargo.toml`.

### Migration Guide

**Before (v0.12.x):**
```toml
[dependencies]
x402-rs = { version = "0.12", features = ["eip155", "solana"] }
```

**After (v1.0.0):**
```toml
[dependencies]
x402-types = "1.0"
x402-chain-eip155 = { version = "1.0", features = ["client"] }
x402-chain-solana = { version = "1.0", features = ["client"] }
```

## [0.12.6] - 2025-01-26

- Added Aptos chain support.

## [0.12.5] - 2025-01-21

- Previous monolithic crate version before workspace refactor.
