//! Utility types and functions for x402.
//!
//! This module provides various helper types used throughout the x402 crate:
//!
//! - [`b64`] - Base64 encoding/decoding utilities
//! - [`lit_str`] - Compile-time string literal types
//! - [`money_amount`] - Human-readable currency amount parsing
//! - [`sig_down`] - Graceful shutdown signal handling
//! - [`telemetry`] - OpenTelemetry tracing setup

pub mod lit_str;
pub mod money_amount;
pub mod sig_down;
pub mod telemetry;

pub use sig_down::*;
pub use telemetry::*;
