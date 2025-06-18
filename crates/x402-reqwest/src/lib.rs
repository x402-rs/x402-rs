//! # x402-request
//!
//! A middleware/extension for [`reqwest`] that adds support for
//! `402 Payment Required` responses using the [x402 protocol](https://x402.org/).
//!
//! This crate allows your HTTP client to transparently detect when a server requires
//! payment, construct a valid x402 payment, and retry the request with the necessary `X-Payment` header attached.
//!
//! ## Features
//! - Seamless integration with [`reqwest`] and [`reqwest_middleware`]
//! - Transparent handling of `402 Payment Required` responses
//! - EIP-712 signing using any [`alloy::Signer`]
//! - Fluent builder pattern for ergonomic usage
//! - Token-specific payment caps and preference lists
//!
//! ## Token Preferences and Spending Limits
//! You can control how the client selects a payment method when multiple options are offered
//! by the server. Using `.prefer(...)`, you can specify a priority list of accepted tokens.
//!
//! You can also enforce safety limits with `.max(...)` to ensure that the client never
//! spends more than a configured amount per token. This helps prevent overpayment or abuse.
//!
//! ```rust,no_run
//! use alloy::signers::local::PrivateKeySigner;
//! use x402_reqwest::{MaxTokenAmountFromAmount, X402Payments};
//! use x402_rs::network::{Network, USDCDeployment};
//!
//! let signer: PrivateKeySigner = "0x...".parse()?;
//! X402Payments::with_signer(signer)
//!     // Example: prefer USDC on Base, and limit payments to 1.00 USDC 
//!     .prefer(USDCDeployment::by_network(Network::Base))
//!     .max(USDCDeployment::by_network(Network::Base).amount("1.00")?)
//! ```
//!
//! ## Examples
//!
//! ### Using [`reqwest::ClientBuilder`]
//! ```rust,no_run
//! use reqwest::ClientBuilder;
//! use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, MaxTokenAmountFromAmount};
//! use alloy::signers::local::PrivateKeySigner;
//! use x402_rs::network::{Network, USDCDeployment};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let signer: PrivateKeySigner = "0x...".parse()?;
//!     let client = ClientBuilder::new()
//!         .with_payments(signer)
//!         .prefer(USDCDeployment::by_network(Network::Base))
//!         .max(USDCDeployment::by_network(Network::Base).amount("1.00")?)
//!         .build()?;
//!
//!     let response = client
//!         .get("https://example.com/protected-endpoint")
//!         .send()
//!         .await?;
//!     // response has been paid for
//!     Ok(())
//! }
//! ```
//!
//! ### Using [`reqwest::Client`]
//! ```rust,no_run
//! use reqwest::Client;
//! use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, MaxTokenAmountFromAmount};
//! use alloy::signers::local::PrivateKeySigner;
//! use x402_rs::network::{Network, USDCDeployment};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let signer: PrivateKeySigner = "0x...".parse()?;
//!     let client = Client::new()
//!         .with_payments(signer)
//!         .prefer(USDCDeployment::by_network(Network::Base))
//!         .max(USDCDeployment::by_network(Network::Base).amount("1.00")?)
//!         .build();
//!
//!     let response = client
//!         .get("https://example.com/protected-endpoint")
//!         .send()
//!         .await?;
//!     // response has been paid for
//!     Ok(())
//! }
//! ```
//!
//! ### Advanced: using [`reqwest_middleware::ClientBuilder`]
//! ```rust,no_run
//! use alloy::signers::local::PrivateKeySigner;
//! use reqwest::Client;
//! use reqwest_middleware as rqm;
//! use x402_reqwest::{MaxTokenAmountFromAmount, X402Payments};
//! use x402_rs::network::{Network, USDCDeployment};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let signer: PrivateKeySigner = "0x...".parse()?;
//!     let client = rqm::ClientBuilder::new(Client::new())
//!         .with(
//!             X402Payments::with_signer(signer)
//!                 .prefer(USDCDeployment::by_network(Network::BaseSepolia))
//!                 .max(USDCDeployment::by_network(Network::BaseSepolia).amount(0.1)?),
//!         )
//!         .build();
//!
//!     let response = client
//!         .get("https://example.com/protected-endpoint")
//!         .send()
//!         .await?;
//!     // response has been paid for
//!     Ok(())
//! }
//! ```
//!
//! ## How It Works
//! When a request receives a `402 Payment Required` response, the middleware:
//! 1. Parses the `Payment-Required` body
//! 2. Selects a compatible payment requirement (based on your preferences)
//! 3. Constructs a signed [`TransferWithAuthorization`] payload
//! 4. Encodes it as a base64 `X-Payment` header
//! 5. Retries the request with that header attached
//!
//! If the response succeeds, it may also include an `X-Payment-Response` header
//! that the server exposes for transparency or logging.
//!
//! ## Crate Layout
//! - [`middleware`] – The core [`X402Payments`] middleware and logic
//! - [`builder`] – Builder traits for attaching `X402Payments` to [`reqwest::Client`] or [`reqwest::ClientBuilder`]
//!
//! ## Related Crates
//! - [`x402-rs`](https://docs.rs/x402-rs) – protocol types and network info
//! - [`x402-axum`](https://docs.rs/x402-axum) – Axum middleware for receiving x402 payments
//!
//! [`x402`]: https://github.com/coinbase/x402
//! [`reqwest`]: https://docs.rs/reqwest
//! [`reqwest_middleware`]: https://docs.rs/reqwest-middleware

mod builder;
mod middleware;

pub use builder::*;
pub use middleware::*;