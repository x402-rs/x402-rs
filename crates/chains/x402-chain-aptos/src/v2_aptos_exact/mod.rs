//! V2 Aptos "exact" payment scheme implementation.
//!
//! This module implements the "exact" payment scheme for Aptos using
//! the V2 x402 protocol. It uses CAIP-2 chain identifiers (aptos:1, aptos:2).
//!
//! # Features
//!
//! - Fungible asset transfers using `0x1::primary_fungible_store::transfer`
//! - Sponsored (gasless) transactions where the facilitator pays gas fees
//! - Transaction simulation before settlement
//! - BCS-encoded transaction validation
//!
//! # Usage
//!
//! ```ignore
//! use x402::scheme::v2_aptos_exact::V2AptosExact;
//! use x402::networks::{KnownNetworkAptos, USDC};
//!
//! // Create a price tag for 1 USDC on Aptos mainnet
//! let usdc = USDC::aptos();
//! let price = V2AptosExact::price_tag(
//!     "0x1234...",  // pay_to address
//!     usdc.amount(1_000_000),  // 1 USDC
//! );
//! ```

pub mod types;

use std::collections::HashMap;
use std::sync::Arc;

use x402_types::proto;
use x402_types::proto::PaymentVerificationError;
use x402_types::proto::v2;
use x402_types::scheme::{
    X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError, X402SchemeId,
};

use aptos_types::account_address::AccountAddress;
use aptos_types::transaction::authenticator::AccountAuthenticator;
use aptos_types::transaction::{EntryFunction, RawTransaction, SignedTransaction};
use move_core_types::identifier::Identifier;
use move_core_types::language_storage::ModuleId;
use x402_types::chain::ChainProviderOps;
use x402_types::util::Base64Bytes;

use crate::chain::AptosChainProvider;
use types::ExactScheme;

pub struct V2AptosExact;

impl X402SchemeId for V2AptosExact {
    fn namespace(&self) -> &str {
        "aptos"
    }

    fn scheme(&self) -> &str {
        ExactScheme.as_ref()
    }
}

pub struct V2AptosExactFacilitator {
    provider: Arc<AptosChainProvider>,
}

impl X402SchemeFacilitatorBuilder<Arc<AptosChainProvider>> for V2AptosExact {
    fn build(
        &self,
        provider: Arc<AptosChainProvider>,
        _config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        Ok(Box::new(V2AptosExactFacilitator { provider }))
    }
}

#[async_trait::async_trait]
impl X402SchemeFacilitator for V2AptosExactFacilitator {
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError> {
        let request = types::VerifyRequest::from_proto(request.clone())?;
        let verification = verify_transfer(&self.provider, &request).await?;
        Ok(v2::VerifyResponse::valid(verification.payer.to_string()).into())
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, X402SchemeFacilitatorError> {
        let request = types::SettleRequest::from_proto(request.clone())?;
        let verification = verify_transfer(&self.provider, &request).await?;
        let payer = verification.payer.to_string();
        let tx_hash = settle_transaction(&self.provider, verification).await?;
        Ok(v2::SettleResponse::Success {
            payer,
            transaction: format!("0x{}", hex::encode(tx_hash)),
            network: self.provider.chain_id().to_string(),
        }
        .into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError> {
        let chain_id = self.provider.chain_id();

        // Include extra.sponsored if the facilitator is configured to sponsor gas
        let extra = if self.provider.sponsor_gas() {
            Some(serde_json::json!({ "sponsored": true }))
        } else {
            None
        };

        let kinds: Vec<proto::SupportedPaymentKind> = vec![proto::SupportedPaymentKind {
            x402_version: proto::v2::X402Version2.into(),
            scheme: ExactScheme.to_string(),
            network: chain_id.to_string(),
            extra,
        }];
        let signers = {
            let mut signers = HashMap::with_capacity(1);
            signers.insert(chain_id, self.provider.signer_addresses());
            signers
        };
        Ok(proto::SupportedResponse {
            kinds,
            extensions: Vec::new(),
            signers,
        })
    }
}

/// Result of verifying an Aptos transfer
pub struct VerifyTransferResult {
    pub payer: AccountAddress,
    pub raw_transaction: RawTransaction,
    pub authenticator_bytes: Vec<u8>,
}

/// Verify an Aptos transfer request
pub async fn verify_transfer(
    provider: &AptosChainProvider,
    request: &types::VerifyRequest,
) -> Result<VerifyTransferResult, PaymentVerificationError> {
    let payload = &request.payment_payload;
    let requirements = &request.payment_requirements;

    // Validate accepted == requirements
    let accepted = &payload.accepted;
    if accepted != requirements {
        return Err(PaymentVerificationError::AcceptedRequirementsMismatch);
    }

    // Validate chain ID
    let chain_id = provider.chain_id();
    let payload_chain_id = &accepted.network;
    if payload_chain_id != &chain_id {
        return Err(PaymentVerificationError::UnsupportedChain);
    }

    // Deserialize transaction
    let transaction_b64 = &payload.payload.transaction;
    let (raw_transaction, authenticator_bytes, entry_function) =
        deserialize_aptos_transaction(transaction_b64)?;

    // Extract sender (payer)
    let payer = raw_transaction.sender();

    // Validate entry function is primary_fungible_store::transfer
    let expected_module = ModuleId::new(
        AccountAddress::ONE,
        Identifier::new("primary_fungible_store").map_err(|e| {
            PaymentVerificationError::InvalidFormat(format!("Invalid module identifier: {}", e))
        })?,
    );
    let expected_function = Identifier::new("transfer").map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!("Invalid function identifier: {}", e))
    })?;

    if entry_function.module() != &expected_module {
        return Err(PaymentVerificationError::InvalidFormat(format!(
            "Invalid module: expected {}, got {}",
            expected_module,
            entry_function.module()
        )));
    }

    if *entry_function.function() != *expected_function {
        return Err(PaymentVerificationError::InvalidFormat(format!(
            "Invalid function: expected {}, got {}",
            expected_function,
            entry_function.function()
        )));
    }

    // Validate function arguments (asset, recipient, amount)
    // primary_fungible_store::transfer has 3 arguments:
    // 1. asset: Object<Metadata> - the fungible asset metadata address
    // 2. to: address - the recipient address
    // 3. amount: u64 - the transfer amount
    let args = entry_function.args();
    if args.len() != 3 {
        return Err(PaymentVerificationError::InvalidFormat(format!(
            "Expected 3 arguments for transfer, got {}",
            args.len()
        )));
    }

    // Parse asset address from first argument (BCS-encoded address)
    let asset_address: AccountAddress = bcs::from_bytes(&args[0]).map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!("Failed to parse asset address: {}", e))
    })?;
    let expected_asset = requirements.asset.inner();
    if &asset_address != expected_asset {
        return Err(PaymentVerificationError::AssetMismatch);
    }

    // Parse recipient address from second argument (BCS-encoded address)
    let recipient_address: AccountAddress = bcs::from_bytes(&args[1]).map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!("Failed to parse recipient address: {}", e))
    })?;
    let expected_recipient = requirements.pay_to.inner();
    if &recipient_address != expected_recipient {
        return Err(PaymentVerificationError::RecipientMismatch);
    }

    // Parse amount from third argument (BCS-encoded u64)
    let amount: u64 = bcs::from_bytes(&args[2]).map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!("Failed to parse amount: {}", e))
    })?;
    let expected_amount: u64 = requirements.amount.parse().map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!("Failed to parse expected amount: {}", e))
    })?;
    if amount != expected_amount {
        return Err(PaymentVerificationError::InvalidPaymentAmount);
    }

    Ok(VerifyTransferResult {
        payer,
        raw_transaction,
        authenticator_bytes,
    })
}

/// Settle the transaction by submitting it to the network
pub async fn settle_transaction(
    provider: &AptosChainProvider,
    verification: VerifyTransferResult,
) -> Result<[u8; 32], PaymentVerificationError> {
    use aptos_crypto::SigningKey;
    use aptos_crypto::ed25519::Ed25519PublicKey;
    use aptos_types::transaction::RawTransactionWithData;

    // Deserialize sender's authenticator
    let sender_authenticator: AccountAuthenticator =
        bcs::from_bytes(&verification.authenticator_bytes).map_err(|e| {
            PaymentVerificationError::InvalidFormat(format!(
                "Failed to deserialize authenticator: {}",
                e
            ))
        })?;

    let signed_txn = if provider.sponsor_gas() {
        // Sponsored transaction: facilitator signs as fee payer
        let fee_payer_address = provider.account_address().ok_or_else(|| {
            PaymentVerificationError::InvalidFormat(
                "Fee payer address not configured for sponsored transaction".to_string(),
            )
        })?;
        let fee_payer_private_key = provider.private_key().ok_or_else(|| {
            PaymentVerificationError::InvalidFormat(
                "Fee payer private key not configured for sponsored transaction".to_string(),
            )
        })?;
        let fee_payer_public_key: Ed25519PublicKey = fee_payer_private_key.into();

        // Create the message that the fee payer needs to sign
        let fee_payer_message = RawTransactionWithData::new_fee_payer(
            verification.raw_transaction.clone(),
            vec![], // No secondary signers
            fee_payer_address,
        );

        // Sign as fee payer
        let fee_payer_signature = fee_payer_private_key
            .sign(&fee_payer_message)
            .map_err(|e| {
                PaymentVerificationError::InvalidSignature(format!(
                    "Failed to sign as fee payer: {}",
                    e
                ))
            })?;

        let fee_payer_authenticator =
            AccountAuthenticator::ed25519(fee_payer_public_key.clone(), fee_payer_signature);

        // Create fee payer signed transaction
        SignedTransaction::new_fee_payer(
            verification.raw_transaction.clone(),
            sender_authenticator,
            vec![], // No secondary signer addresses
            vec![], // No secondary signers
            fee_payer_address,
            fee_payer_authenticator,
        )
    } else {
        // Non-sponsored transaction: client pays own gas, just submit their fully-signed transaction
        // Extract public key and signature from the sender's authenticator
        let (public_key, signature) = match sender_authenticator {
            AccountAuthenticator::Ed25519 {
                public_key,
                signature,
            } => (public_key, signature),
            _ => {
                return Err(PaymentVerificationError::InvalidFormat(
                    "Only Ed25519 signatures are supported for non-sponsored transactions"
                        .to_string(),
                ));
            }
        };

        SignedTransaction::new(verification.raw_transaction.clone(), public_key, signature)
    };

    // Compute transaction hash after signing
    let tx_hash = signed_txn.committed_hash();
    let tx_hash_bytes: [u8; 32] = tx_hash.to_vec().try_into().map_err(|_| {
        PaymentVerificationError::InvalidFormat("Invalid transaction hash".to_string())
    })?;

    // Submit transaction
    provider
        .rest_client()
        .submit_bcs(&signed_txn)
        .await
        .map_err(|e| {
            PaymentVerificationError::TransactionSimulation(format!(
                "Transaction submission failed: {}",
                e
            ))
        })?;

    Ok(tx_hash_bytes)
}

/// Deserialize Aptos transaction from base64-encoded JSON
fn deserialize_aptos_transaction(
    transaction_b64: &str,
) -> Result<(RawTransaction, Vec<u8>, EntryFunction), PaymentVerificationError> {
    // Base64 decode
    let json_bytes = Base64Bytes::from(transaction_b64.as_bytes())
        .decode()
        .map_err(|e| {
            PaymentVerificationError::InvalidFormat(format!("Base64 decode failed: {}", e))
        })?;

    // Parse JSON
    let json_payload: serde_json::Value = serde_json::from_slice(&json_bytes).map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!("JSON parse failed: {}", e))
    })?;

    // Extract transaction and authenticator byte arrays
    let transaction_bytes = json_payload
        .get("transaction")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            PaymentVerificationError::InvalidFormat("Missing transaction field".to_string())
        })?
        .iter()
        .map(|v| v.as_u64().unwrap_or(0) as u8)
        .collect::<Vec<u8>>();

    let authenticator_bytes = json_payload
        .get("senderAuthenticator")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            PaymentVerificationError::InvalidFormat("Missing senderAuthenticator field".to_string())
        })?
        .iter()
        .map(|v| v.as_u64().unwrap_or(0) as u8)
        .collect::<Vec<u8>>();

    // Deserialize RawTransaction from BCS
    // The transaction bytes are a SimpleTransaction which contains RawTransaction + optional fee payer
    // For fee payer transactions, we need to extract just the RawTransaction
    let raw_transaction: RawTransaction = if transaction_bytes.len() > 33 {
        // Check if this might be a fee payer transaction (has Some variant at end)
        let maybe_option_tag = transaction_bytes[transaction_bytes.len() - 33];
        if maybe_option_tag == 1 {
            // Fee payer transaction - extract raw transaction (everything except last 33 bytes)
            let raw_tx_bytes = &transaction_bytes[..transaction_bytes.len() - 33];
            bcs::from_bytes(raw_tx_bytes).map_err(|e| {
                PaymentVerificationError::InvalidFormat(format!(
                    "Failed to deserialize RawTransaction: {}",
                    e
                ))
            })?
        } else {
            bcs::from_bytes(&transaction_bytes).map_err(|e| {
                PaymentVerificationError::InvalidFormat(format!(
                    "Failed to deserialize RawTransaction: {}",
                    e
                ))
            })?
        }
    } else {
        bcs::from_bytes(&transaction_bytes).map_err(|e| {
            PaymentVerificationError::InvalidFormat(format!(
                "Failed to deserialize RawTransaction: {}",
                e
            ))
        })?
    };

    // Clone raw_transaction before consuming it with into_payload
    let raw_transaction_clone = raw_transaction.clone();

    // Extract entry function from payload (consumes raw_transaction)
    let entry_function = match raw_transaction.into_payload() {
        aptos_types::transaction::TransactionPayload::EntryFunction(ef) => ef,
        _ => {
            return Err(PaymentVerificationError::InvalidFormat(
                "Expected EntryFunction payload".to_string(),
            ));
        }
    };

    Ok((raw_transaction_clone, authenticator_bytes, entry_function))
}
