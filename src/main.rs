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

mod chain;
mod config;
mod facilitator_local;
mod handlers;
mod networks;
mod util;

use axum::Router;
use axum::http::Method;
use dotenvy::dotenv;
use std::net::SocketAddr;
use std::process;
use std::sync::Arc;
use tower_http::cors;
use x402_chain_aptos::V2AptosExact;
use x402_chain_eip155::{V1Eip155Exact, V2Eip155Exact};
use x402_chain_solana::{V1SolanaExact, V2SolanaExact};
use x402_types::chain::ChainRegistry;
use x402_types::chain::FromConfig;
use x402_types::scheme::{
    SchemeBlueprints, SchemeRegistry, X402SchemeFacilitator, X402SchemeFacilitatorBuilder,
};

use crate::chain::ChainProvider;
use crate::config::Config;
use crate::facilitator_local::FacilitatorLocal;
use crate::util::{SigDown, Telemetry};

impl X402SchemeFacilitatorBuilder<&ChainProvider> for V1SolanaExact {
    fn build(
        &self,
        provider: &ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let solana_provider = if let ChainProvider::Solana(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V1SolanaExact::build: provider must be a SolanaChainProvider".into());
        };
        self.build(solana_provider, config)
    }
}

impl X402SchemeFacilitatorBuilder<&ChainProvider> for V2SolanaExact {
    fn build(
        &self,
        provider: &ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let solana_provider = if let ChainProvider::Solana(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V2SolanaExact::build: provider must be a SolanaChainProvider".into());
        };
        self.build(solana_provider, config)
    }
}

impl X402SchemeFacilitatorBuilder<&ChainProvider> for V2Eip155Exact {
    fn build(
        &self,
        provider: &ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let eip155_provider = if let ChainProvider::Eip155(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V2Eip155Exact::build: provider must be an Eip155ChainProvider".into());
        };
        self.build(eip155_provider, config)
    }
}

impl X402SchemeFacilitatorBuilder<&ChainProvider> for V2AptosExact {
    fn build(
        &self,
        provider: &ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let aptos_provider = if let ChainProvider::Aptos(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V2AptosExact::build: provider must be an AptosChainProvider".into());
        };
        self.build(aptos_provider, config)
    }
}

impl X402SchemeFacilitatorBuilder<&ChainProvider> for V1Eip155Exact {
    fn build(
        &self,
        provider: &ChainProvider,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let eip155_provider = if let ChainProvider::Eip155(provider) = provider {
            Arc::clone(provider)
        } else {
            return Err("V1Eip155Exact::build: provider must be an Eip155ChainProvider".into());
        };
        self.build(eip155_provider, config)
    }
}

/// Initializes the x402 facilitator server.
///
/// - Loads `.env` variables.
/// - Initializes OpenTelemetry tracing.
/// - Connects to Ethereum providers for supported networks.
/// - Starts an Axum HTTP server with the x402 protocol handlers.
///
/// Binds to the address specified by the `HOST` and `PORT` env vars.
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize rustls crypto provider (ring)
    rustls::crypto::CryptoProvider::install_default(rustls::crypto::ring::default_provider())
        .expect("Failed to initialize rustls crypto provider");

    // Load .env variables
    dotenv().ok();

    let telemetry = Telemetry::new()
        .with_name(env!("CARGO_PKG_NAME"))
        .with_version(env!("CARGO_PKG_VERSION"))
        .register();

    let config = Config::load()?;

    let chain_registry = ChainRegistry::from_config(config.chains()).await?;
    let scheme_blueprints = {
        let blueprints = SchemeBlueprints::new()
            .and_register(V1Eip155Exact)
            .and_register(V2Eip155Exact)
            .and_register(V1SolanaExact)
            .and_register(V2SolanaExact);
        #[cfg(feature = "aptos")]
        let blueprints = blueprints.and_register(V2AptosExact);
        blueprints
    };
    let scheme_registry =
        SchemeRegistry::build(chain_registry, scheme_blueprints, config.schemes());

    let facilitator = FacilitatorLocal::new(scheme_registry);
    let axum_state = Arc::new(facilitator);

    let http_endpoints = Router::new()
        .merge(handlers::routes().with_state(axum_state))
        .layer(telemetry.http_tracing())
        .layer(
            cors::CorsLayer::new()
                .allow_origin(cors::Any)
                .allow_methods([Method::GET, Method::POST])
                .allow_headers(cors::Any),
        );

    let addr = SocketAddr::new(config.host(), config.port());
    tracing::info!("Starting server at http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Failed to bind to {}: {}", addr, e);
            std::process::exit(1);
        });

    let sig_down = SigDown::try_new()?;
    let axum_cancellation_token = sig_down.cancellation_token();
    let axum_graceful_shutdown = async move { axum_cancellation_token.cancelled().await };
    axum::serve(listener, http_endpoints)
        .with_graceful_shutdown(axum_graceful_shutdown)
        .await?;

    Ok(())
}

#[tokio::main]
async fn main() {
    let result = run().await;
    if let Err(e) = result {
        println!("{e}");
        process::exit(1)
    }
}
