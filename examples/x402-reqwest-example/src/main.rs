use alloy_signer_local::PrivateKeySigner;
use dotenvy::dotenv;
use reqwest::Client;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_keypair::{Keypair, Signer};
use std::env;
use std::sync::Arc;
use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, X402Client};
use x402_rs::scheme::v1_eip155_exact::client::V1Eip155ExactClient;
use x402_rs::scheme::v1_solana_exact::client::V1SolanaExactClient;
use x402_rs::scheme::v2_solana_exact::client::V2SolanaExactClient;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let mut x402_client = X402Client::new();

    let signer: PrivateKeySigner = env::var("EVM_PRIVATE_KEY")?.parse()?;
    let signer = Arc::new(signer);
    println!("Signer address: {:?}", signer.address());

    let solana_private_key = env::var("SOLANA_PRIVATE_KEY")?;
    let keypair = Keypair::from_base58_string(solana_private_key.as_str());
    println!("Solana address: {:?}", keypair.pubkey());
    let keypair = Arc::new(keypair);
    let solana_rpc_url = env::var("SOLANA_RPC_URL")?;
    let rpc_client = Arc::new(RpcClient::new(solana_rpc_url.clone()));

    // Register the EVM client with a wildcard pattern to handle all EIP-155 chains
    let x402_client = x402_client
        .register(V1SolanaExactClient::new(
            keypair.clone(),
            Arc::clone(&rpc_client),
        ))
        .register(V2SolanaExactClient::new(keypair, Arc::clone(&rpc_client)))
        .register(V1Eip155ExactClient::new(signer));
    let http_client = Client::new().with_payments(x402_client).build();

    let response = http_client
        .get("http://localhost:3001/protected-route")
        .send()
        .await?;

    println!("Response: {:?}", response.text().await?);

    Ok(())
}
