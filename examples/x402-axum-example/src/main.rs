use alloy_primitives::address;
use axum::Router;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use dotenvy::dotenv;
use solana_pubkey::pubkey;
use std::env;
use tracing::instrument;
use x402_axum::X402Middleware;
use x402_rs::networks::{KnownNetworkEip155, KnownNetworkSolana, USDC};
use x402_rs::scheme::v1_eip155_exact::V1Eip155Exact;
use x402_rs::scheme::v1_solana_exact::V1SolanaExact;
use x402_rs::scheme::v2_eip155_exact::V2Eip155Exact;
use x402_rs::scheme::v2_solana_exact::V2SolanaExact;
use x402_rs::util::Telemetry;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let telemetry = Telemetry::new()
        .with_name(env!("CARGO_PKG_NAME"))
        .with_version(env!("CARGO_PKG_VERSION"))
        .register();

    let facilitator_url =
        env::var("FACILITATOR_URL").unwrap_or("https://facilitator.x402.rs".to_string());

    let x402 = X402Middleware::try_from(facilitator_url)?;

    let app = Router::new()
        .route(
            "/static-price-v1",
            get(my_handler).layer(
                x402.with_price_tag(V1Eip155Exact::price_tag(
                    address!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07"),
                    USDC::base_sepolia().parse("0.01")?,
                ))
                .with_price_tag(V1SolanaExact::price_tag(
                    pubkey!("EGBQqKn968sVv5cQh5Cr72pSTHfxsuzq7o7asqYB5uEV"),
                    USDC::solana().amount(100),
                )),
            ),
        )
        .route(
            "/static-price-v2",
            get(my_handler).layer(
                x402.with_price_tag(V2Eip155Exact::price_tag(
                    address!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07"),
                    USDC::base_sepolia().amount(10),
                ))
                .with_price_tag(V2SolanaExact::price_tag(
                    pubkey!("EGBQqKn968sVv5cQh5Cr72pSTHfxsuzq7o7asqYB5uEV"),
                    USDC::solana().amount(100),
                )),
            ),
        )
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
