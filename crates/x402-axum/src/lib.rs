//! Axum middleware for enforcing [x402](https://www.x402.org) payments on protected routes.
//!
//! This middleware validates incoming payment headers using a configured x402 facilitator,
//! and settles valid payments either before or after request execution (configurable).
//!
//! Returns a `402 Payment Required` response if the request lacks a valid payment.
//!
//! ## Example Usage
//!
//! ```rust
//! use alloy_primitives::address;
//! use axum::{Router, routing::get};
//! use axum::response::IntoResponse;
//! use http::StatusCode;
//! use x402_axum::X402Middleware;
//! use x402_rs::networks::{KnownNetworkEip155, USDC};
//! use x402_rs::scheme::v1_eip155_exact::V1Eip155Exact;
//!
//! let x402 = X402Middleware::new("https://facilitator.x402.rs");
//!
//! let app: Router = Router::new().route(
//!     "/protected",
//!     get(my_handler).layer(
//!         x402.with_price_tag(V1Eip155Exact::price_tag(
//!             address!("0xd8dA6BF26964aF9D7eEd9e03E53415D37aA96045"),
//!             USDC::base_sepolia().parse("0.01").unwrap(),
//!         ))
//!     ),
//! );
//!
//! async fn my_handler() -> impl IntoResponse {
//!     (StatusCode::OK, "This is VIP content!")
//! }
//! ```
//!
//! See [`X402Middleware`] for full configuration options.
//! For low-level interaction with the facilitator, see [`facilitator_client::FacilitatorClient`].
//!
//! ## Protocol Support
//!
//! Supports both V1 and V2 x402 protocols through the [`PaygateProtocol`] trait.
//! The protocol version is determined by the price tag type used.
//!
//! ## Settlement Timing
//!
//! By default, settlement occurs **after** the request is processed. You can change this behavior:
//!
//! - **[`X402Middleware::settle_before_execution`]** - Settle payment **before** request execution.
//!   This prevents issues where failed settlements need retry or authorization expires.
//! - **[`X402Middleware::settle_after_execution`]** - Settle payment **after** request execution (default).
//!   This allows processing the request before committing the payment on-chain.
//!
//! ## Configuration Notes
//!
//! - **[`X402Middleware::with_price_tag`]** sets the assets and amounts accepted for payment.
//! - **[`X402Middleware::with_base_url`]** sets the base URL for computing full resource URLs.
//!   If not set, defaults to `http://localhost/` (avoid in production).
//! - **[`X402Middleware::with_supported_cache_ttl`]** configures the TTL for caching facilitator capabilities.
//! - **[`X402LayerBuilder::with_description`]** is optional but helps the payer understand what is being paid for.
//! - **[`X402LayerBuilder::with_mime_type`]** sets the MIME type of the protected resource (default: `application/json`).
//! - **[`X402LayerBuilder::with_resource`]** explicitly sets the full URI of the protected resource.

pub mod facilitator_client;
pub mod layer;
pub mod paygate;

pub use layer::X402Middleware;
