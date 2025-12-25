mod x402_req;

use alloy_signer_local::PrivateKeySigner;
use dotenvy::dotenv;
use reqwest::Client;
use std::env;

use crate::x402_req::{ReqwestWithPayments, ReqwestWithPaymentsBuild, V2Eip155ExactClient, X402Client};

async fn buy_evm() -> Result<(), Box<dyn std::error::Error>> {
    let signer: PrivateKeySigner = env::var("EVM_PRIVATE_KEY")?.parse()?;
    let x402_client = X402Client::new().register(V2Eip155ExactClient::new(signer));
    let http_client = Client::new().with_payments(x402_client).build();
    // let sender = EvmSenderWallet::new(signer);
    //
    // // Vanilla reqwest
    // let http_client = Client::new()
    //     .with_payments(sender)
    //     .prefer(USDCDeployment::by_network(Network::BaseSepolia))
    //     .max(USDCDeployment::by_network(Network::BaseSepolia).amount(0.1)?)
    //     .build();
    //
    let response = http_client
        .get("http://localhost:3000/protected-route")
        .send()
        .await?;

    println!("Response: {:?}", response.text().await?);

    Ok(())
}

// #[allow(dead_code)] // It is an example!
// async fn buy_solana() -> Result<(), Box<dyn std::error::Error>> {
//     let solana_private_key = env::var("SOLANA_PRIVATE_KEY")?;
//     let keypair = Keypair::from_base58_string(solana_private_key.as_str());
//     let solana_rpc_url = env::var("SOLANA_RPC_URL")?;
//     let rpc_client = RpcClient::new(solana_rpc_url.as_str());
//     let sender = SolanaSenderWallet::new(keypair, rpc_client);
//
//     // Vanilla reqwest
//     let http_client = Client::new()
//         .with_payments(sender)
//         .prefer(USDCDeployment::by_network(Network::Solana))
//         .max(USDCDeployment::by_network(Network::Solana).amount(0.1)?)
//         .build();
//
//     let response = http_client
//         .get("http://localhost:3000/protected-route")
//         .send()
//         .await?;
//
//     println!("Response: {:?}", response.text().await?);
//
//     Ok(())
// }

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenv().ok();

    buy_evm().await
}
