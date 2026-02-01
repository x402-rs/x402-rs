#![cfg_attr(docsrs, feature(doc_auto_cfg))]

//! Reqwest middleware for automatic [x402](https://www.x402.org) payment handling.
//!
//! This crate provides a [`X402Client`] that can be used as a `reqwest` middleware
//! to automatically handle `402 Payment Required` responses. When a request receives
//! a 402 response, the middleware extracts payment requirements, signs a payment,
//! and retries the request with the payment header.
//!
//! ## Quickstart
//!
//! ```rust,ignore
//! use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, X402Client};
//! use x402_chain_eip155::V1Eip155ExactClient;
//! use alloy_signer_local::PrivateKeySigner;
//! use std::sync::Arc;
//! use reqwest::Client;
//!
//! // Create an X402 client and register scheme clients
//! let signer = Arc::new("PRIVATE_KEY".parse::<PrivateKeySigner>().unwrap());
//! let x402_client = X402Client::new()
//!     .register(V1Eip155ExactClient::new(signer));
//!
//! // Build a reqwest client with x402 middleware
//! let http_client = Client::new()
//!     .with_payments(x402_client)
//!     .build();
//!
//! // Use the client - payments are handled automatically
//! let response = http_client
//!     .get("https://api.example.com/protected")
//!     .send()
//!     .await?;
//! ```
//!
//! ## Registering Scheme Clients
//!
//! The [`X402Client`] uses a plugin architecture for supporting different payment schemes.
//! Register scheme clients for each chain/network you want to support:
//!
//! - **[`V1Eip155ExactClient`]** - EIP-155 chains, x402 V1 protocol, "exact" payment scheme
//! - **[`V2Eip155ExactClient`]** - EIP-155 chains, x402 V2 protocol, "exact" payment scheme
//! - **[`V1SolanaExactClient`]** - Solana chains, x402 V1 protocol, "exact" payment scheme
//! - **[`V2SolanaExactClient`]** - Solana chains, x402 V2 protocol, "exact" payment scheme
//!
//! See [`X402Client::register`] for more details on registering scheme clients.
//!
//! ## Payment Selection
//!
//! When multiple payment options are available, the [`X402Client`] uses a [`PaymentSelector`]
//! to choose the best option. By default, it uses [`FirstMatch`] which selects the first
//! matching scheme. You can implement custom selection logic by providing your own selector.
//!
//! See [`X402Client::with_selector`] for custom payment selection.

mod builder;
mod client;

pub use builder::*;
pub use client::*;
