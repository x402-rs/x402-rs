#![cfg_attr(docsrs, feature(doc_auto_cfg))]

//! Core types for the x402 payment protocol.
//!
//! This crate provides the foundational types used throughout the x402 ecosystem
//! for implementing HTTP 402 Payment Required flows. It is designed to be
//! blockchain-agnostic, with chain-specific implementations provided by separate crates.
//!
//! # Overview
//!
//! The x402 protocol enables micropayments over HTTP by leveraging the 402 Payment Required
//! status code. When a client requests a paid resource, the server responds with payment
//! requirements. The client signs a payment authorization, which is verified and settled
//! by a facilitator.
//!
//! # Modules
//!
//! - [`chain`] - Blockchain identifiers and provider abstractions (CAIP-2 chain IDs)
//! - [`config`] - Server configuration, CLI parsing, RPC config, and environment variable resolution
//! - [`facilitator`] - Core trait for payment verification and settlement
//! - [`networks`] - Registry of well-known blockchain networks
//! - [`proto`] - Wire format types for protocol messages (V1 and V2)
//! - [`scheme`] - Payment scheme system for extensible payment methods
//! - [`timestamp`] - Unix timestamp utilities for payment authorization windows
//! - [`util`] - Helper types (base64, string literals, money amounts)
//!
//! # Protocol Versions
//!
//! The crate supports two protocol versions:
//!
//! - **V1** ([`proto::v1`]): Original protocol using network names (e.g., "base-sepolia")
//! - **V2** ([`proto::v2`]): Enhanced protocol using CAIP-2 chain IDs (e.g., "eip155:84532")
//!
//! # Feature Flags
//!
//! - `cli` - Enables CLI argument parsing via clap for configuration loading
//! - `telemetry` - Enables tracing instrumentation for debugging and monitoring

pub mod chain;
pub mod config;
pub mod facilitator;
pub mod networks;
pub mod proto;
pub mod scheme;
pub mod timestamp;
pub mod util;
