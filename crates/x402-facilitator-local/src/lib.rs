#![cfg_attr(docsrs, feature(doc_auto_cfg))]

//! Local facilitator implementation for the x402 payment protocol.
//!
//! This crate provides [`FacilitatorLocal`], a [`Facilitator`](x402_types::facilitator::Facilitator)
//! implementation that validates x402 payment payloads and performs on-chain settlements
//! using registered scheme handlers.
//!
//! # Architecture
//!
//! The local facilitator uses a scheme-based architecture:
//!
//! 1. **Chain Registry**: Manages blockchain providers and connections ([`x402_types::chain::ChainRegistry`])
//! 2. **Scheme Blueprints**: Defines available payment schemes ([`x402_types::scheme::SchemeBlueprints`])
//! 3. **Scheme Registry**: Combines chains and schemes into executable handlers ([`x402_types::scheme::SchemeRegistry`])
//! 4. **FacilitatorLocal**: Routes requests to the appropriate scheme handler ([`FacilitatorLocal`])
//!
//! # Modules
//!
//! - [`facilitator_local`] - Core facilitator implementation
//! - [`handlers`] - HTTP endpoints for the x402 protocol
//! - [`util`] - Utilities for graceful shutdown and telemetry
//!
//! # Example
//!
//! ```ignore
//! use x402_facilitator_local::{FacilitatorLocal, handlers};
//! use x402_types::chain::ChainRegistry;
//! use x402_types::scheme::{SchemeBlueprints, SchemeRegistry};
//! use x402_chain_eip155::{V1Eip155Exact, V2Eip155Exact};
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Initialize chain registry
//!     let chain_registry = ChainRegistry::from_config(&chains_config).await?;
//!
//!     // Register schemes
//!     let scheme_blueprints = SchemeBlueprints::new()
//!         .and_register(V1Eip155Exact)
//!         .and_register(V2Eip155Exact);
//!
//!     // Build scheme registry
//!     let scheme_registry = SchemeRegistry::build(
//!         chain_registry,
//!         scheme_blueprints,
//!         &schemes_config,
//!     );
//!
//!     // Create facilitator
//!     let facilitator = FacilitatorLocal::new(scheme_registry);
//!     let state = Arc::new(facilitator);
//!
//!     // Create HTTP routes
//!     let app = axum::Router::new()
//!         .merge(handlers::routes().with_state(state));
//!
//!     // Run server
//!     let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
//!     axum::serve(listener, app).await?;
//!
//!     Ok(())
//! }
//! ```

pub mod facilitator_local;
pub mod handlers;
pub mod util;

pub use facilitator_local::*;
pub use handlers::*;
