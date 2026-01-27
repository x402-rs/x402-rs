//! Core Rust implementation of the [x402 protocol](https://www.x402.org).
//!
//! This crate provides the foundational data structures, protocol types, and a reference
//! facilitator implementation for on-chain verification and settlement of x402 payments.
//!
//! # Overview
//!
//! The x402 protocol enables HTTP-native payments using the `402 Payment Required` status code.
//! This crate supports both EVM-compatible chains (via EIP-155) and Solana, with multiple
//! protocol versions (V1 and V2) and payment schemes.
//!
//! # Roles
//!
//! The crate is designed for reuse across all x402 roles:
//!
//! - **Facilitator**: A server that verifies and settles x402 payments on-chain.
//!   See [`facilitator`] for the trait definition and [`facilitator_local`] for the
//!   reference implementation.
//!
//! - **Seller**: A payment-gated service that requires payment for access to resources.
//!   Use the [`proto`] module for protocol types and [`scheme`] for payment scheme definitions.
//!
//! - **Buyer/Client**: A client that constructs and submits x402-compliant payments.
//!   See [`scheme::client`] for client-side payment handling.
//!
//! # Modules
//!
//! - [`chain`] — Blockchain-specific types and providers for EIP-155 (EVM) and Solana chains.
//! - [`config`] — Configuration types for the facilitator server, including chain and scheme settings.
//! - [`facilitator`] — The [`Facilitator`](facilitator::Facilitator) trait for payment verification and settlement.
//! - [`facilitator_local`] — Reference implementation of the facilitator using on-chain verification.
//! - [`handlers`] — HTTP endpoint handlers for the facilitator server (verify, settle, supported).
//! - [`networks`] — Registry of well-known blockchain networks and CAIP-2 chain identifiers.
//! - [`proto`] — Protocol types for x402 V1 and V2, including payment payloads and requirements.
//! - [`scheme`] — Payment scheme implementations (e.g., `exact` scheme for EIP-155 and Solana).
//! - [`timestamp`] — Unix timestamp type for payment authorization windows.
//! - [`util`] — Utility types including base64 encoding, telemetry, and signal handling.
//!
//! # Feature Highlights
//!
//! - **Multi-chain support**: EVM chains via EIP-155 and Solana
//! - **Protocol versions**: Both x402 V1 and V2 protocols
//! - **Payment schemes**: Extensible scheme system with built-in `exact` scheme
//! - **CAIP-2 identifiers**: Standard chain-agnostic blockchain identification
//! - **OpenTelemetry**: Built-in tracing and metrics support
//!
//! # Example
//!
//! For a complete facilitator server example, see the `x402-axum-example` in the examples directory.
//! For client-side payment handling, see the `x402-reqwest` crate.

pub mod chain;
pub mod config;
pub mod facilitator_local;
pub mod handlers;
pub mod networks;
pub mod util;
