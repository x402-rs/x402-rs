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
use x402_chain_eip155::chain::{AssetTransferMethod, Eip155TokenDeployment};
use x402_chain_eip155::{KnownNetworkEip155, V1Eip155Exact, V2Eip155Exact, V2Eip155Upto};
use x402_chain_solana::{KnownNetworkSolana, V1SolanaExact, V2SolanaExact};
use x402_types::networks::USDC;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    init_tracing();

    let usdc_base_sepolia = USDC::base_sepolia();
    let usdc_base_sepolia_permit2 = Eip155TokenDeployment {
        chain_reference: usdc_base_sepolia.chain_reference,
        address: usdc_base_sepolia.address,
        decimals: usdc_base_sepolia.decimals,
        transfer_method: AssetTransferMethod::Permit2,
    };

    let facilitator_url =
        env::var("FACILITATOR_URL").unwrap_or("https://facilitator.x402.rs".to_string());
    let port = env::var("PORT").unwrap_or("3000".to_string());

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
                    USDC::solana_devnet().amount(100),
                )),
            ),
        )
        .route(
            "/static-price-v2",
            get(static_price_v2_handler).layer(
                x402.with_price_tag(V2Eip155Exact::price_tag(
                    address!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07"),
                    USDC::base_sepolia().amount(10u64),
                ))
                .with_price_tag(V2SolanaExact::price_tag(
                    pubkey!("EGBQqKn968sVv5cQh5Cr72pSTHfxsuzq7o7asqYB5uEV"),
                    USDC::solana_devnet().amount(100),
                )),
            ),
        )
        .route(
            "/static-price-v2-permit2",
            get(static_price_v2_permit2_handler).layer(x402.with_price_tag(
                V2Eip155Exact::price_tag(
                    address!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07"),
                    usdc_base_sepolia_permit2.amount(10u64),
                ),
            )),
        )
        // Dynamic pricing: adjust price based on request parameters
        // GET /dynamic-price-v2 -> 100 units
        // GET /dynamic-price-v2?discount -> 50 units (discounted)
        .route(
            "/dynamic-price-v2",
            get(my_handler).layer(x402.with_dynamic_price(|_headers, uri, _base_url| {
                // Check if "discount" query parameter is present (before async block)
                let has_discount = uri.query().map(|q| q.contains("discount")).unwrap_or(false);
                let amount: u64 = if has_discount { 50 } else { 100 };

                async move {
                    vec![
                        // V2 EIP155 (Base Sepolia) price tag
                        V2Eip155Exact::price_tag(
                            address!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07"),
                            USDC::base_sepolia().amount(amount),
                        ),
                        // V2 Solana price tag
                        V2SolanaExact::price_tag(
                            pubkey!("EGBQqKn968sVv5cQh5Cr72pSTHfxsuzq7o7asqYB5uEV"),
                            USDC::solana_devnet().amount(amount),
                        ),
                    ]
                }
            })),
        )
        // Conditional free access: bypass payment when "free" query parameter is present
        // GET /conditional-free-v2 -> requires payment (402)
        // GET /conditional-free-v2?free -> bypasses payment, returns content directly
        //
        // This demonstrates returning an empty price tags vector to skip payment enforcement.
        // Useful for implementing free tiers, promotional access, or conditional pricing.
        .route(
            "/conditional-free-v2",
            get(my_handler).layer(x402.with_dynamic_price(|_headers, uri, _base_url| {
                // Check if "free" query parameter is present - if so, bypass payment
                let is_free = uri.query().map(|q| q.contains("free")).unwrap_or(false);

                async move {
                    if is_free {
                        // Return empty vector to bypass payment enforcement entirely.
                        // The middleware will forward the request directly to the handler
                        // without requiring any payment.
                        vec![]
                    } else {
                        // Normal pricing - payment required
                        vec![
                            V2Eip155Exact::price_tag(
                                address!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07"),
                                USDC::base_sepolia().amount(100u64),
                            ),
                            V2SolanaExact::price_tag(
                                pubkey!("EGBQqKn968sVv5cQh5Cr72pSTHfxsuzq7o7asqYB5uEV"),
                                USDC::solana_devnet().amount(100),
                            ),
                        ]
                    }
                }
            })),
        )
        // TODO CONTINUE Set price (and feature-based docs.rs)
        .route(
            "/eip155-upto",
            get(eip155_upto_handler).layer(x402.with_price_tag(V2Eip155Upto::price_tag(
                address!("0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07"),
                usdc_base_sepolia_permit2.amount(10u64),
            ))),
        );

    tracing::info!("Using facilitator on {}", x402.facilitator_url());

    let bind_address = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(bind_address)
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
async fn static_price_v2_handler() -> impl IntoResponse {
    (StatusCode::OK, "VIP content from /static-price-v2")
}

#[instrument(skip_all)]
async fn static_price_v2_permit2_handler() -> impl IntoResponse {
    (StatusCode::OK, "VIP content from /static-price-v2-permit2")
}

#[instrument(skip_all)]
async fn eip155_upto_handler() -> impl IntoResponse {
    (StatusCode::OK, "VIP content from /eip155-upto")
}

fn init_tracing() {
    use tracing_subscriber::{EnvFilter, fmt};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("trace"));

    fmt()
        .with_env_filter(filter)
        .with_target(false) // cleaner logs
        .with_level(true)
        .with_thread_ids(false)
        .with_thread_names(false)
        .init();
}
