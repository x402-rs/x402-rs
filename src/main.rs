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

use axum::http::Method;
use axum::routing::post;
use axum::{Extension, Router, routing::get};
use dotenvy::dotenv;
use opentelemetry::trace::Status;
use std::env;
use std::net::SocketAddr;
use tower_http::cors;
use tower_http::trace::TraceLayer;
use tracing_opentelemetry::OpenTelemetrySpanExt;

use crate::facilitator_local::FacilitatorLocal;
use crate::provider_cache::ProviderCache;
use crate::telemetry::Telemetry;

mod chain;
mod facilitator;
mod facilitator_local;
mod handlers;
mod network;
mod provider_cache;
mod telemetry;
mod timestamp;
mod types;

/// Initializes the x402 facilitator server.
///
/// - Loads `.env` variables.
/// - Initializes OpenTelemetry tracing.
/// - Connects to Ethereum providers for supported networks.
/// - Starts an Axum HTTP server with the x402 protocol handlers.
///
/// Binds to the address specified by the `HOST` and `PORT` env vars.
#[tokio::main]
async fn main() {
    // Load .env variables
    dotenv().ok();

    let _telemetry = Telemetry::new()
        .with_name(env!("CARGO_PKG_NAME"))
        .with_version(env!("CARGO_PKG_VERSION"))
        .register();

    let provider_cache = ProviderCache::from_env().await;
    // Abort if we can't initialize Ethereum providers early
    if let Err(e) = provider_cache {
        tracing::error!("Failed to create Ethereum providers: {}", e);
        std::process::exit(1);
    }
    let facilitator = FacilitatorLocal::new(provider_cache.unwrap());

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" })) // Liveness or sanity check route
        .route("/verify", get(handlers::get_verify_info))
        .route("/verify", post(handlers::post_verify))
        .route("/settle", get(handlers::get_settle_info))
        .route("/settle", post(handlers::post_settle))
        .route("/supported", get(handlers::get_supported))
        .layer(Extension(facilitator))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    tracing::info_span!(
                        "http_request",
                        otel.kind = "server",
                        otel.name = %format!("{} {}", request.method(), request.uri()),
                        method = %request.method(),
                        uri = %request.uri(),
                        version = ?request.version(),
                    )
                })
                .on_response(
                    |response: &axum::http::Response<_>,
                     latency: std::time::Duration,
                     span: &tracing::Span| {
                        span.record("status", tracing::field::display(response.status()));
                        span.record("latency", tracing::field::display(latency.as_millis()));
                        span.record(
                            "http.status_code",
                            tracing::field::display(response.status().as_u16()),
                        );

                        // OpenTelemetry span status
                        if response.status().is_success() {
                            span.set_status(Status::Ok);
                        } else {
                            span.set_status(Status::error(
                                response
                                    .status()
                                    .canonical_reason()
                                    .unwrap_or("unknown")
                                    .to_string(),
                            ));
                        }

                        tracing::info!(
                            "status={} elapsed={}ms",
                            response.status().as_u16(),
                            latency.as_millis()
                        );
                    },
                ),
        )
        .layer(
            cors::CorsLayer::new()
                .allow_origin(cors::Any)
                .allow_methods([Method::GET, Method::POST])
                .allow_headers(cors::Any),
        );

    let host = env::var("HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = env::var("PORT")
        .ok()
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(8080);

    let addr = SocketAddr::from((host.parse::<std::net::IpAddr>().unwrap(), port));
    tracing::info!("Starting server at http://{}", addr);

    let listener = match tokio::net::TcpListener::bind(addr).await {
        Ok(listener) => listener,
        Err(e) => {
            tracing::error!("Failed to bind to {}: {}", addr, e);
            std::process::exit(1);
        }
    };

    if let Err(e) = axum::serve(listener, app).await {
        tracing::error!("Server error: {}", e);
    }
}
