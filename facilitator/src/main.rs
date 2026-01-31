//! x402 Facilitator HTTP server binary.
//!
//! This is the main entry point for the x402 facilitator server, a production-ready
//! HTTP server that implements the x402 payment protocol for blockchain-based micropayments.
//!
//! # Usage
//!
//! ```bash
//! # Run with default configuration (config.json)
//! cargo run --package facilitator
//!
//! # Run with custom configuration
//! cargo run --package facilitator -- --config /path/to/config.json
//!
//! # Run with telemetry enabled
//! cargo run --package facilitator --features telemetry
//! ```
//!
//! # Configuration
//!
//! The server loads configuration from a JSON file. See [`config`](crate::config) module
//! for the configuration format and environment variables.
//!
//! # Supported Blockchains
//!
//! - **EIP-155 (EVM)**: Ethereum, Base, Polygon, and other EVM-compatible chains
//! - **Solana**: Mainnet, Devnet, and custom clusters
//! - **Aptos**: Mainnet and testnet
//!
//! # Architecture
//!
//! The binary is organized into modules:
//! - [`chain`](crate::chain) - Blockchain provider abstractions
//! - [`config`](crate::config) - Configuration loading and validation
//! - [`run`](crate::run) - HTTP server initialization and request handling
//! - [`schemes`](crate::schemes) - Payment scheme registration

mod chain;
mod config;
mod run;
mod schemes;

use std::process;

use crate::run::run;

#[tokio::main]
async fn main() {
    let result = run().await;
    if let Err(e) = result {
        println!("{e}");
        process::exit(1)
    }
}
