use alloy::signers::local::PrivateKeySigner;
use dotenvy::dotenv;
use reqwest::Client;
use std::env;
use x402_reqwest::{MaxTokenAmountFromAmount, ReqwestWithPayments, ReqwestWithPaymentsBuild};
use x402_rs::network::{Network, USDCDeployment};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    let signer: PrivateKeySigner = env::var("PRIVATE_KEY")?.parse()?;

    // Vanilla reqwest
    let http_client = Client::new()
        .with_payments(signer)
        .prefer(USDCDeployment::by_network(Network::BaseSepolia))
        .max(USDCDeployment::by_network(Network::BaseSepolia).amount(0.1)?)
        .build();

    let response = http_client
        .get("http://localhost:3000/protected-route")
        .send()
        .await?;

    println!("Response: {:?}", response.text().await?);

    Ok(())
}
