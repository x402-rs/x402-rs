use alloy_signer_local::PrivateKeySigner;
use dotenvy::dotenv;
use reqwest::Client;
use std::env;
use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, X402Client};
use x402_rs::scheme::v1_eip155_exact::client::V1Eip155ExactClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let signer: PrivateKeySigner = env::var("EVM_PRIVATE_KEY")?.parse()?;
    // let signer = Arc::new(signer); TODO
    println!("Signer address: {:?}", signer.address());

    // Register the EVM client with a wildcard pattern to handle all EIP-155 chains
    let x402_client = X402Client::new().register(V1Eip155ExactClient::from(signer));
    let http_client = Client::new().with_payments(x402_client).build();

    let response = http_client
        .get("http://localhost:3001/protected-route")
        .send()
        .await?;

    println!("Response: {:?}", response.text().await?);

    Ok(())
}

// TODO Solana and other schemes
