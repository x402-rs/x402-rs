//! TRON chain support for the x402 payment protocol.
//!
//! This crate provides an implementation of the x402 payment protocol for the TRON blockchain.
//! TRON uses TIP-712 (identical to EIP-712) for signing, making the authorization struct
//! byte-compatible with EIP-155 at the EIP-712 layer. Key differences from EVM:
//!
//! - Addresses are Base58Check on the wire but EVM hex in the authorization payload
//! - No ERC-1271/EIP-6492 — only EOA ecrecover
//! - Settlement via TronGrid HTTP API (not alloy providers)
//!
//! # Feature Flags
//!
//! - `facilitator` — Enables verification and settlement logic
//! - `telemetry` — Enables tracing support

pub mod chain;
pub mod networks;
pub mod v2_tron_exact;

pub use chain::TRON_NAMESPACE;
pub use networks::{KnownNetworkTron, USDT};
pub use v2_tron_exact::V2TronExact;
