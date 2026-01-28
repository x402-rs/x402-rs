//! This module provides various helper types used throughout the x402-facilitator-local crate:
//!
//! - [`sig_down`] - Graceful shutdown signal handling
//! - [`telemetry`] - OpenTelemetry tracing setup (requires `telemetry` feature)

pub mod sig_down;
#[cfg(feature = "telemetry")]
pub mod telemetry;

pub use sig_down::*;
#[cfg(feature = "telemetry")]
pub use telemetry::*;
