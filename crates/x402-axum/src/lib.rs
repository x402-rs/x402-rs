//! Axum middleware and helpers for enforcing [x402](https://www.x402.org) payments.
//!
//! This crate provides an [`X402Middleware`] Axum layer for protecting routes with payment enforcement,
//! as well as a [`FacilitatorClient`] for communicating with remote x402 facilitators.
//!
//! ## Quickstart
//!
//! ```rust,no_run
//! use axum::{Router, routing::get, Json};
//! use axum::response::IntoResponse;
//! use http::StatusCode;
//! use serde_json::json;
//! use x402_axum::{X402Middleware, IntoPriceTag};
//! use x402_rs::network::{Network, USDCDeployment};
//!
//! let x402 = X402Middleware::try_from("https://facilitator.example.com/").unwrap();
//! // You can construct `TokenAsset` manually. Here we use known USDC on Base Sepolia
//! let usdc = USDCDeployment::by_network(Network::BaseSepolia).pay_to("0xADDRESS");
//!
//! let app: Router = Router::new().route(
//!     "/paywall",
//!     get(my_handler).layer(
//!         x402.with_description("Premium Content")
//!             .with_price_tag(usdc.amount(0.025).unwrap()),
//!     ),
//! );
//!
//! async fn my_handler() -> impl IntoResponse {
//!     (StatusCode::OK, Json(json!({ "hello": "world" })))
//! }
//! ```
//! See [`X402Middleware`] for full configuration options.
//! For low-level interaction with the facilitator, see [`facilitator_client::FacilitatorClient`].
//!
//! ## Defining Prices
//!
//! To define price tags for your protected routes, see the [`price`] module.
//! It provides builder-style helpers like [`IntoPriceTag`] and types like [`PriceTag`]
//! for working with tokens, networks, and payment amounts.

pub mod facilitator_client;
pub mod layer;
pub mod price;

pub use layer::X402Middleware;
pub use price::*;
