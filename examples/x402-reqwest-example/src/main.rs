use alloy::signers::local::PrivateKeySigner;
use dotenvy::dotenv;
use reqwest::Client;
use solana_sdk::signature::Keypair;
use std::env;
use x402_reqwest::chains::evm::EvmSenderWallet;
use x402_reqwest::chains::solana::SolanaSenderWallet;
use x402_reqwest::{MaxTokenAmountFromAmount, ReqwestWithPayments, ReqwestWithPaymentsBuild};
use x402_rs::network::{Network, USDCDeployment};

async fn buy_evm() -> Result<(), Box<dyn std::error::Error>> {
    let signer: PrivateKeySigner = env::var("EVM_PRIVATE_KEY")?.parse()?;
    let sender = EvmSenderWallet::new(signer);

    // Vanilla reqwest
    let http_client = Client::new()
        .with_payments(sender)
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

#[allow(dead_code)] // It is an example!
async fn buy_solana() -> Result<(), Box<dyn std::error::Error>> {
    let solana_private_key = env::var("SOLANA_PRIVATE_KEY")?;
    let keypair = Keypair::from_base58_string(solana_private_key.as_str());
    let solana_rpc_url = env::var("SOLANA_RPC_URL")?;
    let rpc_client = solana_client::rpc_client::RpcClient::new(solana_rpc_url.as_str());
    let sender = SolanaSenderWallet::new(keypair, rpc_client);

    // Vanilla reqwest
    let http_client = Client::new()
        .with_payments(sender)
        .prefer(USDCDeployment::by_network(Network::Solana))
        .max(USDCDeployment::by_network(Network::Solana).amount(0.1)?)
        .build();

    let response = http_client
        .get("http://localhost:3000/protected-route")
        .send()
        .await?;

    println!("Response: {:?}", response.text().await?);

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    buy_evm().await
}
