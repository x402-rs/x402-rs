use crate::provider_cache::ProviderCache;
use axum::http::Method;
use axum::routing::post;
use axum::{routing::get, Extension, Router};
use dotenvy::dotenv;
use std::env;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::cors;
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use crate::trace_id::{TraceId, TraceIdLayer};

mod facilitator;
mod handlers;
mod network;
mod provider_cache;
mod trace_id;
mod types;

#[tokio::main]
async fn main() {
    // Load .env variables
    dotenv().ok();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer())
        .init();

    let provider_cache = ProviderCache::from_env().await;
    if let Err(e) = provider_cache {
        tracing::error!("Failed to create Ethereum providers: {}", e);
        std::process::exit(1);
    }
    let provider_cache = Arc::new(provider_cache.unwrap());

    let app = Router::new()
        .route("/", get(|| async { "Hello, World!" }))
        .route("/verify", get(handlers::get_verify_info))
        .route("/verify", post(handlers::post_verify))
        .route("/settle", get(handlers::get_settle_info))
        .route("/settle", post(handlers::post_settle))
        .route("/supported", get(handlers::get_supported))
        .layer(Extension(provider_cache))
        .layer(
            TraceLayer::new_for_http()
                .make_span_with(|request: &axum::http::Request<_>| {
                    let trace_id = request
                        .extensions()
                        .get::<TraceId>()
                        .map(|id| id.0.to_string())
                        .unwrap_or_else(|| "unknown".to_string());

                    tracing::info_span!(
                        "http_request",
                        method = %request.method(),
                        uri = %request.uri(),
                        version = ?request.version(),
                        trace_id = %trace_id,
                    )
                })
                .on_response(
                    |response: &axum::http::Response<_>,
                     latency: std::time::Duration,
                     span: &tracing::Span| {
                        span.record("status", tracing::field::display(response.status()));
                        span.record("latency", tracing::field::display(latency.as_millis()));
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
        )
        .layer(TraceIdLayer);

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
