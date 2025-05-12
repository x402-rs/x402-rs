//! x402 Facilitator core modules.
//!
//! This crate implements the server-side logic for an x402 facilitator,
//! including Ethereum network integration, caching of providers,
//! OpenTelemetry instrumentation, and type-safe handling of payment
//! payloads and responses.
//!
//! - `facilitator`: core logic for handling payment verification and settlement
//! - `network`: definitions of supported networks and chain metadata
//! - `provider_cache`: shared cache of Ethereum providers per network
//! - `telemetry`: OpenTelemetry setup and subscriber initialization
//! - `types`: all x402 protocol structures, payload formats, and verification types

pub mod facilitator;
pub mod network;
pub mod provider_cache;
pub mod telemetry;
pub mod types;
