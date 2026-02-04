use alloy_signer_local::PrivateKeySigner;
use dotenvy::dotenv;
use reqwest::Client;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_keypair::Keypair;
use std::env;
use std::sync::Arc;
use x402_chain_eip155::{V1Eip155ExactClient, V2Eip155ExactClient};
use x402_chain_solana::{V1SolanaExactClient, V2SolanaExactClient};
use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, X402Client};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    let mut x402_client = X402Client::new();
    // Register eip155 "exact" scheme
    {
        let signer: Option<PrivateKeySigner> = env::var("EVM_PRIVATE_KEY")
            .ok()
            .and_then(|key| key.parse().ok());
        if let Some(signer) = signer {
            println!("Using EVM signer address: {:?}", signer.address());
            let signer = Arc::new(signer);
            x402_client = x402_client
                .register(V1Eip155ExactClient::new(signer.clone()))
                .register(V2Eip155ExactClient::new(signer));
            println!("Enabled eip155 exact scheme")
        }
    };

    // Register solana "exact" scheme
    {
        let keypair = env::var("SOLANA_PRIVATE_KEY")
            .ok()
            .map(|v| Keypair::from_base58_string(&v));
        let rpc_client = env::var("SOLANA_RPC_URL").ok().map(RpcClient::new);
        if let Some((keypair, rpc_client)) = keypair.zip(rpc_client) {
            let keypair = Arc::new(keypair);
            let rpc_client = Arc::new(rpc_client);
            x402_client = x402_client
                .register(V1SolanaExactClient::new(
                    keypair.clone(),
                    rpc_client.clone(),
                ))
                .register(V2SolanaExactClient::new(keypair, rpc_client));
            println!("Enabled solana exact scheme")
        }
    }

    let http_client = Client::new().with_payments(x402_client).build();

    let endpoint = env::var("ENDPOINT").unwrap_or("http://localhost:3000/protected-route".to_string());

    let response = http_client
        .get(endpoint)
        .send()
        .await?;

    println!("Response: {:?}", response.text().await?);

    Ok(())
}
