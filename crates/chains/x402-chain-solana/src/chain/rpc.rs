#![cfg(feature = "client")]

use solana_account::Account;
use solana_client::client_error::ClientError;
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_client::rpc_config::RpcSimulateTransactionConfig;
use solana_client::rpc_response::{RpcPrioritizationFee, RpcResult, RpcSimulateTransactionResult};
use solana_message::Hash;
use solana_pubkey::Pubkey;
use solana_transaction::versioned::VersionedTransaction;

pub trait RpcClientLike {
    fn get_account(
        &self,
        pubkey: &Pubkey,
    ) -> impl Future<Output = Result<Account, ClientError>> + Send;
    fn simulate_transaction_with_config(
        &self,
        transaction: &VersionedTransaction,
        config: RpcSimulateTransactionConfig,
    ) -> impl Future<Output = RpcResult<RpcSimulateTransactionResult>> + Send;
    fn get_recent_prioritization_fees(
        &self,
        addresses: &[Pubkey],
    ) -> impl Future<Output = Result<Vec<RpcPrioritizationFee>, ClientError>> + Send;
    fn get_latest_blockhash(&self) -> impl Future<Output = Result<Hash, ClientError>> + Send;
}

impl<Container: AsRef<RpcClient>> RpcClientLike for Container {
    fn get_account(
        &self,
        pubkey: &Pubkey,
    ) -> impl Future<Output = Result<Account, ClientError>> + Send {
        RpcClient::get_account(self.as_ref(), pubkey)
    }
    fn simulate_transaction_with_config(
        &self,
        transaction: &VersionedTransaction,
        config: RpcSimulateTransactionConfig,
    ) -> impl Future<Output = RpcResult<RpcSimulateTransactionResult>> + Send {
        RpcClient::simulate_transaction_with_config(self.as_ref(), transaction, config)
    }
    fn get_recent_prioritization_fees(
        &self,
        addresses: &[Pubkey],
    ) -> impl Future<Output = Result<Vec<RpcPrioritizationFee>, ClientError>> + Send {
        RpcClient::get_recent_prioritization_fees(self.as_ref(), addresses)
    }
    fn get_latest_blockhash(&self) -> impl Future<Output = Result<Hash, ClientError>> + Send {
        RpcClient::get_latest_blockhash(self.as_ref())
    }
}
