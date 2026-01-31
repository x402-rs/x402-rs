//! Utility modules for the x402-facilitator-local crate.
//!
//! This module provides various helper types used throughout the crate:
//!
//! | Module | Description | Feature |
//! |--------|-------------|---------|
//! | [`sig_down`] | Graceful shutdown signal handling | - |
//! | [`telemetry`] | OpenTelemetry tracing and metrics setup | `telemetry` |
//!
//! # Example
//!
//! ```ignore
//! use x402_facilitator_local::util::SigDown;
//!
//! let sig_down = SigDown::try_new()?;
//! let token = sig_down.cancellation_token();
//! ```

pub mod sig_down;
#[cfg(feature = "telemetry")]
pub mod telemetry;

pub use sig_down::*;
#[cfg(feature = "telemetry")]
pub use telemetry::*;
