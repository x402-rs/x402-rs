//! x402 Facilitator Server
//!
//! A production-ready HTTP server implementing the [x402](https://www.x402.org) payment protocol.
//!
//! This crate provides a complete, runnable facilitator that supports multiple blockchain
//! networks (EVM/EIP-155, Solana, Aptos) and can verify and settle payments on-chain.
//!
//! # Modules
//!
//! | Module | Description |
//! |--------|-------------|
//! | [`chain`] | Blockchain provider abstractions for EVM, Solana, and Aptos |
//! | [`config`] | Configuration types and loading |
//! | [`run`] | Main server initialization and runtime |
//! | [`schemes`] | Scheme builder implementations for supported payment schemes |
//!
//! # Running the Server
//!
//! ```bash
//! # Run with default configuration
//! cargo run --package facilitator
//!
//! # Run with telemetry
//! cargo run --package facilitator --features telemetry
//!
//! # Run with custom config
//! cargo run --package facilitator -- --config /path/to/config.json
//! ```

pub mod chain;
pub mod config;
pub mod run;
pub mod schemes;

pub use run::run;
