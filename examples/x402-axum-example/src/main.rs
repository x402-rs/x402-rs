use crate::x402::middleware::X402Middleware;
use crate::x402::v1_eip155_exact::V1Eip155ExactSchemePriceTag;
use crate::x402::v1_solana_exact::V1SolanaExactSchemePriceTag;
use axum::Router;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use dotenvy::dotenv;
use std::env;
use tracing::instrument;
// TODO Kill re-exports or make them more direct, like x402_rs::macro::address and ::pubkey
use x402_rs::__reexports::alloy_primitives::address;
use x402_rs::__reexports::solana_pubkey::pubkey;
use x402_rs::chain::solana::Address;
use x402_rs::networks::{KnownNetworkEip155, KnownNetworkSolana, USDC};
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

    let x402 = X402Middleware::try_from(facilitator_url)?;

    let app = Router::new()
        .route(
            "/protected-route",
            get(my_handler).layer(
                x402.with_price_tag(V1Eip155ExactSchemePriceTag {
                    pay_to: address!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07").into(),
                    asset: USDC::base_sepolia().amount(10),
                })
                .with_price_tag(V1SolanaExactSchemePriceTag {
                    pay_to: pubkey!("EGBQqKn968sVv5cQh5Cr72pSTHfxsuzq7o7asqYB5uEV").into(),
                    asset: USDC::solana().amount(100),
                }),
            ),
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
