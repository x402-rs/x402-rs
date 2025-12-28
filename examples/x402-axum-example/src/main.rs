use axum::Router;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use dotenvy::dotenv;
use opentelemetry::trace::Status;
use serde_json::json;
use std::env;
use tower_http::trace::TraceLayer;
use tracing::instrument;
use tracing_opentelemetry::OpenTelemetrySpanExt;
use x402_rs::util::Telemetry;
mod x402;

#[tokio::main]
async fn main() {
    dotenv().ok();

    let _telemetry = Telemetry::new()
        .with_name(env!("CARGO_PKG_NAME"))
        .with_version(env!("CARGO_PKG_VERSION"))
        .register();

    let facilitator_url =
        env::var("FACILITATOR_URL").unwrap_or_else(|_| "https://facilitator.x402.rs".to_string());

}
