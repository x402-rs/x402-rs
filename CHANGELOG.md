# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [2.0.0] - 2026-06-16

### Breaking Changes

- `x402-chain-eip155`: `AssetTransferMethod::Permit2` changed from a unit variant to a struct variant `Permit2 { name: String, version: String }`. Any code that pattern-matches on `AssetTransferMethod::Permit2` must be updated to bind or ignore the new `name` and `version` fields. The wire format now requires `name` and `version` fields in Permit2 payment payloads.
- `x402-types`: `PaymentPayload::extensions` field type changed from `Option<serde_json::Value>` to `ExtensionsJson` (non-optional). Code that accessed `extensions` as `Option<serde_json::Value>` must be updated to use the new `ExtensionsJson` API.
- `x402-types`: `MoneyAmount::parse` now rejects inputs with embedded non-numeric text (e.g. `"1abc"`, `"USD 10"` previously accepted by stripping non-numeric characters are now errors). Currency prefixes `$` and `€` and comma separators remain supported.

### Added

- `x402-types`: New `ExtensionsJson` type — a typed JSON-object map for optional protocol extension data, used in `PaymentPayload` and `PaymentRequired`. Supports typed `insert<T>` / `get<T>` methods keyed by `T::EXTENSION_KEY`.
- `x402-types`: New `ExtensionKey` trait — associates a Rust type with its wire-format string key in the `extensions` map.
- `x402-types`: `PaymentRequired` now carries an `extensions` field (`ExtensionsJson`) for server-provided protocol extension declarations.
- `x402-chain-eip155`: New `Eip2612GasSponsoring` extension with dedicated client and server types, allowing clients to request EIP-2612 permit-based gas sponsoring from the facilitator.
- `x402-chain-eip155`: New `DoRead2612Nonce` trait and multiple provider implementations for reading EIP-2612 nonces on-chain.
- `x402-chain-eip155`: `EIP2612ProviderLike` now supports reading ERC-20 allowances; `try_eip2612_gas_sponsoring` checks existing allowance before creating a permit.
- `x402-chain-eip155`: Full Rust client support for the EIP-155 **"upto"** payment scheme (`V2Eip155UptoClient`), including provider-backed nonce reading and generic provider support.
- `x402-axum`: New `with_extension` method on `X402Middleware` and `X402LayerBuilder` to declare V2 protocol extensions in `PaymentRequired.extensions`.
- New example `x402-reqwest-upto-eip155` demonstrating the EIP-155 "upto" payment scheme client.

### Changed

- Example `x402-reqwest-example` renamed to `x402-reqwest-exact` to better reflect the payment scheme it demonstrates.

### Fixed

- `x402-types`: `MoneyAmount::parse` now correctly validates format — leading `$`/`€` prefixes and comma-separated thousands are accepted; inputs containing embedded alphabetic text are rejected with `MoneyAmountParseError::InvalidFormat`.

## [1.5.6] - 2026-06-03

### Changed

- `x402-chain-eip155`: Internal cleanup — removed unused `EOASignatureExt` import from the EIP-2612 facilitator module.

## [1.5.5] - 2026-06-03

### Added

- `x402-chain-eip155`: Implemented `settleWithPermit` and `assert_onchain_upto_permit2_with_eip2612` for on-chain EIP-2612 gas-sponsored Permit2 settlement.
- `x402-chain-eip155`: Added `Eip2612GasSponsoringInfo` struct and `eip2612_gas_sponsoring` support in `V2Eip155Upto` facilitator configuration.
- `x402-chain-eip155`: Added `extensions` field to `UptoSupportedExtra` for communicating facilitator-supported features to clients.
- `x402-chain-eip155`: Replaced `x402BasePermit2Proxy` with `x402UptoPermit2Proxy` ABI and updated internal types accordingly.
- `x402-chain-eip155`: Implemented `From<StructuredSignature>` for `alloy::Bytes`.

### Changed

- `x402-chain-eip155`: `V2Eip155ExactFacilitatorExtra::extensions` field changed from `Option<Vec<String>>` to `Vec<String>` — callers constructing this struct must update accordingly.

## [1.5.4] - 2026-06-01

### Added

- `x402-chain-eip155`: Random facilitator address selection in the `v2_eip155_upto` module using the `rand` crate.
- `x402-chain-eip155`: `UptoSupportedExtra` struct is now generic over the `facilitator_address` field type for increased flexibility.

### Fixed

- `x402-chain-eip155`: Fixed upto scheme to correctly use the facilitator address from server-provided metadata.

## [1.5.3] - 2026-06-01

### Added

- `x402-chain-eip155`: Added `UptoSupportedExtra` struct for describing server-provided facilitator metadata in the upto scheme.
- `x402-chain-eip155`: Enforced facilitator authorization in Permit2 witness validation, hardening the upto scheme against unauthorized settlement.

### Changed

- `x402-chain-eip155`: Renamed internal `HavingEip155SignerAddresses` trait to `Eip155SignerAddresses` for consistency.
- `x402-chain-eip155`: Refactored facilitator address handling across upto and exact facilitators to use `UptoSupportedExtra` deserialization.

## [1.5.2] - 2026-05-30

### Added

- `x402-axum`: `SettleResponse` is now injected as a request extension after successful settlement, giving handlers access to settlement metadata (e.g. transaction hash, payer address).

### Changed

- `x402-axum`: Enhanced logging in payment settlement paths for improved observability.

## [1.5.1] - 2026-05-29

### Changed

- `x402-chain-eip155`: Added optional `from` field to `MetaTransaction` and a `with_from()` builder method, allowing callers to override the sender address. `Eip155MetaTransactionProvider::send_transaction` now uses `tx.from` when set, falling back to `next_signer_address()`.
- `x402-chain-eip155`: Refactored internal transaction construction in `v1_eip155_exact` and `v2_eip155_exact` facilitators to use the `MetaTransaction::new()` constructor instead of struct-literal syntax, eliminating boilerplate and aligning with the new `from` field default.

## [1.5.0] - 2026-05-29

### Changed

- `x402-chain-eip155`: Bumped Alloy dependencies to 2.0.
- `x402-chain-eip155`: Bumped `rand` dependency to 0.10 and updated imports to use the new `RngExt` module across EIP-155 clients.
- `x402-chain-eip155`: Changed `REQUIRED_CONTRACT_ADDRESSES` from `const` to `static` in `provider.rs`.
- `x402-facilitator-local`: Bumped OpenTelemetry dependencies to 0.32.
- `x402-chain-solana`: Upgraded `spl-token-2022` to v11.0.0.
- Raised minimum supported Rust version to 1.93.0.
- Various transitive dependency version bumps across the workspace.

## [1.4.11] - 2026-05-27

### Added

- `x402-axum`: Added `X402Middleware::from_facilitator` constructor for building middleware directly from a facilitator URL string.
- `x402-chain-eip155`: Added Radius Network and Radius Testnet chain configurations with SBC as the default stablecoin.

## [1.4.10] - 2026-05-14

### Fixed

- Minor community patch (PR #89).

## [1.4.9] - 2026-05-07

### Fixed

- `x402-facilitator-local`: Corrected error type reported for unsupported payment schemes from `Verification` to `Settlement`.

## [1.4.8] - 2026-05-06

### Added

- `x402-facilitator-local`: Implemented `AsJsonValue` trait for `FacilitatorLocalError` to allow structured error serialization.

## [1.4.7] - 2026-04-24

### Added

- `x402-facilitator-local`: Added `FacilitatorContract` trait to decouple facilitator implementations from specific request/response types, improving extensibility.

## [1.4.6] - 2026-04-14

### Added

- `x402-chain-eip155`: Required contract addresses are now validated during provider initialization, surfacing misconfiguration early at startup.
- `x402-types`: Added `DecimalU256` type for decimal string serialization, used in `PaymentRequirements`.

### Changed

- `x402-facilitator-local`: Renamed `error_reason_details` to `error_message` in scheme handler error responses for clarity.

## [1.4.5] - 2026-03-14

### Fixed
- Validate `SettleResponse.success` before serving resources in paygate to prevent serving protected content on failed settlement (PR #74, fixes #65)

## [1.4.4] - 2026-03-14

### Changed
- Made `resource` fields optional in `paygate.rs`, `v2.rs`, `v2_eip155_exact`, and `v2_solana_exact` to use `Option<ResourceInfo>`, improving flexibility and consistency.

## [1.4.3] - 2026-03-10

### Added

- `x402-chain-eip155`: Added EIP-2612 gas sponsoring support for Permit2-based payments with new `Permit2PaymentPayloadExt` trait for unified EIP-2612 gas sponsoring handling.
- `x402-chain-eip155`: Added new `EOASignature` type for improved signature handling.
- `docs`: Added x402 specification v1 and v2 documentation.

### Changed

- `x402-chain-eip155`: Consolidated Permit2 settlement logic with shared execution flow, eliminating duplication in signature and EIP-712 handling between exact/upto schemes.
- `x402-chain-eip155`: Updated `X402ExactPermit2Proxy.json` to current SDK version.

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
