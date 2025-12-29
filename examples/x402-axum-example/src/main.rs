use crate::x402::middleware::{V1Eip155ExactSchemePriceTag, X402};
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
use x402_rs::__reexports::alloy_primitives::address;
use x402_rs::networks::{USDC, KnownNetworkEip155};
use x402_rs::util::Telemetry;

mod x402;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let telemetry = Telemetry::new()
        .with_name(env!("CARGO_PKG_NAME"))
        .with_version(env!("CARGO_PKG_VERSION"))
        .register();

    let facilitator_url =
        env::var("FACILITATOR_URL").unwrap_or_else(|_| "https://facilitator.x402.rs".to_string());

    let x402 = X402::try_from(facilitator_url)?; //.with_base_url(url::Url::parse("https://localhost:3000/").unwrap());
    // let usdc_base_sepolia = USDCDeployment::by_network(Network::BaseSepolia)
    //     .pay_to(address_evm!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07"));
    // let usdc_solana = USDCDeployment::by_network(Network::Solana)
    //     .pay_to(address_sol!("EGBQqKn968sVv5cQh5Cr72pSTHfxsuzq7o7asqYB5uEV"));

    let k = x402.clone();
    // .with_description("Premium API - Discoverable")
    // .with_mime_type("application/json");

    let app = Router::new()
        .route(
            "/protected-route",
            get(my_handler).layer(x402.with_price_tag(V1Eip155ExactSchemePriceTag {
                pay_to: address!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07").into(),
                asset: USDC::base_sepolia().amount(10)
            })), //.layer(
                             // x402.with_description("Premium API - Discoverable")
                             //     .with_mime_type("application/json")
                             //     .with_price_tag()
                             //     .accept()
                             // .with_input_schema(serde_json::json!({
                             //     "type": "http",
                             //     "method": "GET",
                             //     "discoverable": true,
                             //     "description": "Access premium content"
                             // }))
                             // .with_output_schema(serde_json::json!({
                             //     "type": "string",
                             //     "description": "VIP content response"
                             // }))
                             // .with_price_tag(usdc_solana.amount(0.0025).unwrap())
                             // .or_price_tag(usdc_base_sepolia.amount(0.0025).unwrap()),
                             //),
        )
        // .route(
        //     "/api/weather",
        //     get(weather_handler).layer(
        //         x402.with_description("Weather API - Public endpoint with query params")
        //             .with_mime_type("application/json")
        //             .with_input_schema(serde_json::json!({
        //                 "type": "http",
        //                 "method": "GET",
        //                 "discoverable": true,
        //                 "queryParams": {
        //                     "location": {
        //                         "type": "string",
        //                         "description": "City name or coordinates",
        //                         "required": true
        //                     },
        //                     "units": {
        //                         "type": "string",
        //                         "enum": ["metric", "imperial"],
        //                         "default": "metric"
        //                     }
        //                 }
        //             }))
        //             .with_output_schema(serde_json::json!({
        //                 "type": "object",
        //                 "properties": {
        //                     "temperature": { "type": "number", "description": "Current temperature" },
        //                     "conditions": { "type": "string", "description": "Weather conditions" },
        //                     "humidity": { "type": "number", "description": "Humidity percentage" }
        //                 },
        //                 "required": ["temperature", "conditions"]
        //             }))
        //             .with_price_tag(usdc_base_sepolia.amount(0.001).unwrap()),
        //     ),
        // )
        // .route(
        //     "/api/internal",
        //     get(internal_handler).layer(
        //         x402.with_description("Internal API - Private endpoint")
        //             .with_mime_type("application/json")
        //             .with_input_schema(serde_json::json!({
        //                 "type": "http",
        //                 "method": "GET",
        //                 "discoverable": false,
        //                 "description": "Internal admin functions - direct access only"
        //             }))
        //             .with_output_schema(serde_json::json!({
        //                 "type": "object",
        //                 "properties": {
        //                     "status": { "type": "string" }
        //                 }
        //             }))
        //             .with_price_tag(usdc_base_sepolia.amount(1.00).unwrap()),
        //     ),
        // )
        .layer(telemetry.http_tracing());

    tracing::info!("Using facilitator on {}", x402.facilitator_url());

    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .expect("Can not start server");
    tracing::info!("Listening on {}", listener.local_addr().unwrap());
    axum::serve(listener, app).await.unwrap();

    Ok(())
}

#[instrument(skip_all)]
async fn my_handler() -> impl IntoResponse {
    (StatusCode::OK, "This is a VIP content!")
}

#[instrument(skip_all)]
async fn weather_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        axum::Json(json!({
            "temperature": 72,
            "conditions": "sunny",
            "humidity": 45
        })),
    )
}

#[instrument(skip_all)]
async fn internal_handler() -> impl IntoResponse {
    (
        StatusCode::OK,
        axum::Json(json!({
            "status": "admin_access_granted"
        })),
    )
}
