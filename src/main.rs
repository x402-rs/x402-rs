//! x402 Facilitator HTTP entrypoint.
//!
//! This binary launches an Axum-based HTTP server that exposes the x402 protocol interface
//! for payment verification and settlement via Ethereum-compatible networks.
//!
//! Endpoints:
//! - `GET /verify` – Supported verification schema
//! - `POST /verify` – Verify a payment payload against requirements
//! - `GET /settle` – Supported settlement schema
//! - `POST /settle` – Settle an accepted payment payload on-chain
//! - `GET /supported` – List supported payment kinds (version/scheme/network)
//!
//! This server includes:
//! - OpenTelemetry tracing via `TraceLayer`
//! - CORS support for cross-origin clients
//! - Ethereum provider cache for per-network RPC routing
//!
//! Environment:
//! - `.env` values loaded at startup
//! - `HOST`, `PORT` control binding address
//! - `OTEL_*` variables enable tracing to systems like Honeycomb

mod config;
mod p1;
mod telemetry;

use crate::config::Config;
use crate::p1::chain::ChainRegistry;
use crate::telemetry::Telemetry;
use axum::Router;
use axum::http::Method;
use dotenvy::dotenv;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors;

/// Initializes the x402 facilitator server.
///
/// - Loads `.env` variables.
/// - Initializes OpenTelemetry tracing.
/// - Connects to Ethereum providers for supported networks.
/// - Starts an Axum HTTP server with the x402 protocol handlers.
///
/// Binds to the address specified by the `HOST` and `PORT` env vars.
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env variables
    dotenv().ok();

    let telemetry = Telemetry::new()
        .with_name(env!("CARGO_PKG_NAME"))
        .with_version(env!("CARGO_PKG_VERSION"))
        .register();

    let config = Config::load().unwrap_or_else(|e| {
        tracing::error!("Failed to load configuration: {}", e);
        std::process::exit(1);
    });

    let chain_registry = ChainRegistry::from_config(&config.chains()).await?;

    println!("{:?}", chain_registry);

    // let provider_cache = ProviderCache::from_config(config.chains()).await;
    // // Abort if we can't initialise Ethereum providers early
    // let provider_cache = match provider_cache {
    //     Ok(provider_cache) => provider_cache,
    //     Err(e) => {
    //         tracing::error!("Failed to create Ethereum providers: {}", e);
    //         std::process::exit(1);
    //     }
    // };
    // let facilitator = FacilitatorLocal::new(provider_cache);
    // let axum_state = Arc::new(facilitator);
    //
    // let http_endpoints = Router::new()
    //     .merge(handlers::routes().with_state(axum_state))
    //     .layer(telemetry.http_tracing())
    //     .layer(
    //         cors::CorsLayer::new()
    //             .allow_origin(cors::Any)
    //             .allow_methods([Method::GET, Method::POST])
    //             .allow_headers(cors::Any),
    //     );
    //
    // let addr = SocketAddr::new(config.host(), config.port());
    // tracing::info!("Starting server at http://{}", addr);
    //
    // let listener = tokio::net::TcpListener::bind(addr)
    //     .await
    //     .unwrap_or_else(|e| {
    //         tracing::error!("Failed to bind to {}: {}", addr, e);
    //         std::process::exit(1);
    //     });
    //
    // let sig_down = SigDown::try_new()?;
    // let axum_cancellation_token = sig_down.cancellation_token();
    // let axum_graceful_shutdown = async move { axum_cancellation_token.cancelled().await };
    // axum::serve(listener, http_endpoints)
    //     .with_graceful_shutdown(axum_graceful_shutdown)
    //     .await?;

    Ok(())
}
