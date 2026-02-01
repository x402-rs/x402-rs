#![cfg_attr(docsrs, feature(doc_auto_cfg))]

//! Aptos chain support for the x402 payment protocol.
//!
//! This crate provides implementations of the x402 payment protocol for the Aptos blockchain.
//! It currently supports the V2 protocol with the "exact" payment scheme based on fungible
//! asset transfers with sponsored (gasless) transactions.
//!
//! # Features
//!
//! - **V2 Protocol Support**: Implements V2 protocol with CAIP-2 chain ID addressing
//! - **Fungible Asset Payments**: Token transfers using `0x1::primary_fungible_store::transfer`
//! - **Sponsored Transactions**: Facilitator pays gas fees for user transactions
//! - **Transaction Simulation**: Pre-flight validation before settlement
//! - **Balance Verification**: On-chain balance checks before settlement
//!
//! # Architecture
//!
//! The crate is organized into several modules:
//!
//! - [`chain`] - Core Aptos chain types, providers, and configuration
//! - [`v2_aptos_exact`] - V2 protocol implementation with CAIP-2 chain IDs
//!
//! # Feature Flags
//!
//! - `facilitator` - Facilitator-side payment verification and settlement
//! - `telemetry` - OpenTelemetry tracing support
//!
//! # Usage Examples
//!
//! ## Facilitator: Verifying and Settling
//!
//! ```ignore
//! use x402_chain_aptos::{V2AptosExact, AptosChainProvider};
//! use x402_types::scheme::X402SchemeFacilitatorBuilder;
//!
//! let provider = AptosChainProvider::from_config(&config).await?;
//! let facilitator = V2AptosExact.build(provider, None)?;
//!
//! // Verify payment
//! let verify_response = facilitator.verify(&verify_request).await?;
//!
//! // Settle payment
//! let settle_response = facilitator.settle(&settle_request).await?;
//! ```
//!
//! # Sponsored Transactions
//!
//! Aptos payments use sponsored transactions where the facilitator acts as the fee payer.
//! The client creates and signs a transaction, and the facilitator adds its signature
//! as the sponsor before submitting it on-chain.

pub mod chain;
pub mod v2_aptos_exact;

mod networks;
pub use networks::*;

pub use v2_aptos_exact::V2AptosExact;
