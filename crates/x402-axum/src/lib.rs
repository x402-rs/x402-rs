//! Axum middleware for enforcing [x402](https://www.x402.org) payments on protected routes.
//!
//! This middleware validates incoming payment headers using a configured x402 facilitator,
//! and settles valid payments either before or after request execution (configurable).
//!
//! Returns a `402 Payment Required` response if the request lacks a valid payment.
//!
//! ## Example Usage
//!
//! ```rust,no_run
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
//!             "0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07".parse().unwrap(),
//!             USDC::base_sepolia().parse("0.01")?,
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
//! - **[`X402LayerBuilder::with_description`]** is optional but helps the payer understand what is being paid for.
//! - **[`X402LayerBuilder::with_mime_type`]** sets the MIME type of the protected resource (default: `application/json`).
//! - **[`X402LayerBuilder::with_resource`]** explicitly sets the full URI of the protected resource.

pub mod facilitator_client;
pub mod layer;
pub mod paygate;

pub use layer::X402Middleware;
