//! Rust implementation of the x402 protocol.
//!
//! This crate provides the core data structures and logic for working with the x402 protocol,
//! including a reference facilitator implementation for on-chain verification and settlement.
//!
//! It is designed for reuse across all x402 roles:
//! - _Facilitator_: a server that verifies and settles x402 payments (see [`facilitator`] and [`facilitator_local`])
//! - _Seller_: a payment-gated service that consumes shared types from [`types`]
//! - _Buyer_: a client that constructs and submits x402-compliant payments
//!
//! Modules:
//! - [`facilitator`] — defines the [`facilitator::Facilitator`] trait used to validate and settle x402 payments.
//! - [`facilitator_local`] — a concrete implementation of [`facilitator::Facilitator`].
//! - [`network`] — enumerates supported Ethereum-compatible networks and known token deployments.
//! - [`provider_cache`] — dynamic initialization and caching of Ethereum JSON-RPC providers.
//! - [`telemetry`] — OpenTelemetry instrumentation setup for tracing and observability.
//! - [`types`] — all shared x402 protocol structures and payload formats.

pub mod chain;
pub mod facilitator;
pub mod facilitator_local;
pub mod from_env;
pub mod handlers;
pub mod network;
pub mod provider_cache;
pub mod sig_down;
pub mod telemetry;
pub mod timestamp;
pub mod types;

// Hidden re-exports just for macro expansion.
#[doc(hidden)]
pub mod __reexports {
    pub use alloy;
    pub use solana_sdk;
}
