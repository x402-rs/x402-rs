use alloy_signer_local::PrivateKeySigner;
use dotenvy::dotenv;
use reqwest::Client;
use std::env;
use x402_chain_eip155::V2Eip155UptoClient;
use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, X402Client};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();
    dotenvy::from_path(format!("{}/.env", env!("CARGO_MANIFEST_DIR"))).ok();

    let signer: PrivateKeySigner = env::var("EVM_PRIVATE_KEY")
        .expect("EVM_PRIVATE_KEY must be set")
        .parse()?;
    println!("Using EVM signer address: {:?}", signer.address());

    let rpc_url = env::var("EVM_RPC_URL").unwrap_or_else(|_| "https://mainnet.base.org".into());
    let x402_client =
        X402Client::new().register(V2Eip155UptoClient::new(signer).with_provider(rpc_url));
    let http_client = Client::new().with_payments(x402_client).build();

    let endpoint =
        env::var("ENDPOINT").unwrap_or_else(|_| "http://localhost:3000/eip155-upto".to_string());

    let response = http_client.get(endpoint).send().await?;

    println!("Status: {}", response.status());
    println!("Response: {:?}", response.text().await?);

    Ok(())
}
