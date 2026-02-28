use aptos_types::transaction::authenticator::AccountAuthenticator;
use aptos_types::transaction::{EntryFunction, RawTransaction, SignedTransaction};
use move_core_types::account_address::AccountAddress;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use x402_types::chain::ChainProviderOps;
use x402_types::proto;
use x402_types::proto::{PaymentVerificationError, v2};
use x402_types::scheme::{
    X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError,
};
use x402_types::util::Base64Bytes;

use crate::V2AptosExact;
use crate::chain::AptosChainProvider;
use crate::chain::types::Address;
use crate::v2_aptos_exact::types;
use crate::v2_aptos_exact::types::ExactScheme;

/// Maximum gas amount allowed for sponsored transactions to prevent gas draining.
const MAX_GAS_AMOUNT: u64 = 500_000;

/// Buffer in seconds before expiration to ensure transaction has time to execute.
const EXPIRATION_BUFFER_SECONDS: u64 = 5;

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
            transaction: tx_hash,
            network: self.provider.chain_id().to_string(),
        }
        .into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError> {
        let chain_id = self.provider.chain_id();

        // Include extra.feePayer if the facilitator is configured to sponsor gas
        let extra = if self.provider.sponsor_gas() {
            self.provider.account_address().map(|addr| {
                serde_json::json!({ "feePayer": Address::new(addr).to_string() })
            })
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

/// Result of deserializing an Aptos transaction from the payment payload.
#[derive(Debug)]
struct DeserializedAptosTransaction {
    raw_transaction: RawTransaction,
    fee_payer_address: Option<AccountAddress>,
    authenticator_bytes: Vec<u8>,
    entry_function: EntryFunction,
}

/// Result of verifying an Aptos transfer.
pub struct VerifyTransferResult {
    pub payer: AccountAddress,
    pub raw_transaction: RawTransaction,
    pub fee_payer_address: Option<AccountAddress>,
    pub authenticator_bytes: Vec<u8>,
}

/// Mirror struct for accessing private fields of RawTransaction via BCS deserialization.
/// The field order must exactly match `RawTransaction`'s BCS layout.
#[derive(serde::Deserialize)]
struct RawTransactionFields {
    sender: AccountAddress,
    #[allow(dead_code)]
    sequence_number: u64,
    #[allow(dead_code)]
    payload: aptos_types::transaction::TransactionPayload,
    max_gas_amount: u64,
    #[allow(dead_code)]
    gas_unit_price: u64,
    expiration_timestamp_secs: u64,
    chain_id: aptos_types::chain_id::ChainId,
}

/// Verify an Aptos transfer request.
pub async fn verify_transfer(
    provider: &AptosChainProvider,
    request: &types::VerifyRequest,
) -> Result<VerifyTransferResult, PaymentVerificationError> {
    let payload = &request.payment_payload;
    let requirements = &request.payment_requirements;

    // 1. Validate accepted == requirements
    let accepted = &payload.accepted;
    if accepted != requirements {
        return Err(PaymentVerificationError::AcceptedRequirementsMismatch);
    }

    // 2. Validate network/scheme match
    let chain_id = provider.chain_id();
    let payload_chain_id = &accepted.network;
    if payload_chain_id != &chain_id {
        return Err(PaymentVerificationError::UnsupportedChain);
    }

    // 3. Fee payer managed by facilitator check
    let is_sponsored = requirements
        .extra
        .as_ref()
        .and_then(|e| e.fee_payer.as_ref())
        .is_some();

    if is_sponsored {
        let fee_payer_str = requirements
            .extra
            .as_ref()
            .and_then(|e| e.fee_payer.as_ref())
            .map(|fp| fp.to_string())
            .unwrap_or_default();
        let signer_addresses = provider.signer_addresses();
        if !signer_addresses.contains(&fee_payer_str) {
            return Err(PaymentVerificationError::InvalidFormat(
                "fee_payer_not_managed_by_facilitator".to_string(),
            ));
        }
    }

    // 4. Deserialize transaction
    let transaction_b64 = &payload.payload.transaction;
    let deserialized = deserialize_aptos_transaction(transaction_b64)?;

    // Extract sender (payer)
    let payer = deserialized.raw_transaction.sender();

    // Access RawTransaction fields via BCS re-deserialization
    let raw_tx_bytes = bcs::to_bytes(&deserialized.raw_transaction).map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!(
            "Failed to serialize RawTransaction: {}",
            e
        ))
    })?;
    let raw_fields: RawTransactionFields = bcs::from_bytes(&raw_tx_bytes).map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!(
            "Failed to deserialize RawTransaction fields: {}",
            e
        ))
    })?;

    // 5. Chain ID in transaction matches provider
    let expected_chain_id = provider.chain_reference().chain_id();
    let tx_chain_id = raw_fields.chain_id.id();
    if tx_chain_id != expected_chain_id {
        return Err(PaymentVerificationError::ChainIdMismatch);
    }

    // 6. Sender-authenticator matching for Ed25519
    let sender_authenticator: AccountAuthenticator =
        bcs::from_bytes(&deserialized.authenticator_bytes).map_err(|e| {
            PaymentVerificationError::InvalidFormat(format!(
                "Failed to deserialize authenticator: {}",
                e
            ))
        })?;
    if let AccountAuthenticator::Ed25519 {
        ref public_key, ..
    } = sender_authenticator
    {
        use aptos_types::transaction::authenticator::AuthenticationKey;
        let auth_key = AuthenticationKey::ed25519(public_key);
        let derived_address = auth_key.account_address();
        if derived_address != payer {
            return Err(PaymentVerificationError::InvalidSignature(
                "invalid_exact_aptos_payload_sender_authenticator_mismatch".to_string(),
            ));
        }
    }

    // 7. Max gas amount for sponsored transactions
    if is_sponsored && raw_fields.max_gas_amount > MAX_GAS_AMOUNT {
        return Err(PaymentVerificationError::InvalidFormat(format!(
            "invalid_exact_aptos_payload_gas_too_high: {} > {}",
            raw_fields.max_gas_amount, MAX_GAS_AMOUNT
        )));
    }

    // 8. Fee payer address in transaction matches requirements
    if is_sponsored {
        let expected_fee_payer: AccountAddress = *requirements
            .extra
            .as_ref()
            .and_then(|e| e.fee_payer.as_ref())
            .map(|fp| fp.inner())
            .ok_or_else(|| {
                PaymentVerificationError::InvalidFormat(
                    "fee payer required for sponsored transaction".to_string(),
                )
            })?;

        match deserialized.fee_payer_address {
            Some(tx_fee_payer) if tx_fee_payer == expected_fee_payer => {}
            _ => {
                return Err(PaymentVerificationError::InvalidFormat(
                    "invalid_exact_aptos_payload_fee_payer_mismatch".to_string(),
                ));
            }
        }
    }

    // 9. SECURITY: Prevent facilitator from signing away its own tokens
    if is_sponsored {
        let sender_str = Address::new(payer).to_string();
        let signer_addresses = provider.signer_addresses();
        if signer_addresses.contains(&sender_str) {
            return Err(PaymentVerificationError::InvalidFormat(
                "invalid_exact_aptos_payload_fee_payer_transferring_funds".to_string(),
            ));
        }
    }

    // 10. Expiration check with buffer
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| {
            PaymentVerificationError::InvalidFormat(format!("System time error: {}", e))
        })?
        .as_secs();
    if raw_fields.expiration_timestamp_secs < now + EXPIRATION_BUFFER_SECONDS {
        return Err(PaymentVerificationError::Expired);
    }

    // 11. Entry function validation — accept both primary_fungible_store::transfer
    //     and fungible_asset::transfer
    let entry_function = &deserialized.entry_function;

    let module_address = *entry_function.module().address();
    let module_name = entry_function.module().name().to_string();
    let function_name = entry_function.function().to_string();

    let is_primary_fungible_store = module_address == AccountAddress::ONE
        && module_name == "primary_fungible_store"
        && function_name == "transfer";

    let is_fungible_asset = module_address == AccountAddress::ONE
        && module_name == "fungible_asset"
        && function_name == "transfer";

    if !is_primary_fungible_store && !is_fungible_asset {
        return Err(PaymentVerificationError::InvalidFormat(format!(
            "invalid_exact_aptos_payload_wrong_function: {}::{}::{}",
            module_address, module_name, function_name
        )));
    }

    // 12. Type args count == 1
    if entry_function.ty_args().len() != 1 {
        return Err(PaymentVerificationError::InvalidFormat(format!(
            "invalid_exact_aptos_payload_wrong_type_args: expected 1, got {}",
            entry_function.ty_args().len()
        )));
    }

    // 13. Validate function arguments (asset, recipient, amount)
    let args = entry_function.args();
    if args.len() != 3 {
        return Err(PaymentVerificationError::InvalidFormat(format!(
            "Expected 3 arguments for transfer, got {}",
            args.len()
        )));
    }

    // 14. Asset address
    let asset_address: AccountAddress = bcs::from_bytes(&args[0]).map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!("Failed to parse asset address: {}", e))
    })?;
    let expected_asset = requirements.asset.inner();
    if &asset_address != expected_asset {
        return Err(PaymentVerificationError::AssetMismatch);
    }

    // 15. Recipient address
    let recipient_address: AccountAddress = bcs::from_bytes(&args[1]).map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!(
            "Failed to parse recipient address: {}",
            e
        ))
    })?;
    let expected_recipient = requirements.pay_to.inner();
    if &recipient_address != expected_recipient {
        return Err(PaymentVerificationError::RecipientMismatch);
    }

    // 16. Amount
    let amount: u64 = bcs::from_bytes(&args[2]).map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!("Failed to parse amount: {}", e))
    })?;
    let expected_amount: u64 = requirements.amount.parse().map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!("Failed to parse expected amount: {}", e))
    })?;
    if amount != expected_amount {
        return Err(PaymentVerificationError::InvalidPaymentAmount);
    }

    // 17. Balance check via REST API view function
    let balance = query_fungible_asset_balance(
        provider,
        &raw_fields.sender,
        expected_asset,
    )
    .await?;
    if balance < expected_amount {
        return Err(PaymentVerificationError::InsufficientFunds);
    }

    // 18. Transaction simulation
    simulate_transaction(provider, &deserialized).await?;

    Ok(VerifyTransferResult {
        payer,
        raw_transaction: deserialized.raw_transaction,
        fee_payer_address: deserialized.fee_payer_address,
        authenticator_bytes: deserialized.authenticator_bytes,
    })
}

/// Query the fungible asset balance for an owner via the Aptos REST API `/view` endpoint.
///
/// Calls `0x1::primary_fungible_store::balance` as a view function using
/// the SDK's built-in `rest_client.view()` method.
async fn query_fungible_asset_balance(
    provider: &AptosChainProvider,
    owner: &AccountAddress,
    asset: &AccountAddress,
) -> Result<u64, PaymentVerificationError> {
    use aptos_rest_client::aptos_api_types::{EntryFunctionId, MoveType, ViewRequest};

    let view_request = ViewRequest {
        function: "0x1::primary_fungible_store::balance"
            .parse::<EntryFunctionId>()
            .map_err(|e| {
                PaymentVerificationError::InvalidFormat(format!(
                    "Failed to parse view function id: {}",
                    e
                ))
            })?,
        type_arguments: vec![MoveType::Struct(
            "0x1::fungible_asset::Metadata".parse().map_err(|e| {
                PaymentVerificationError::InvalidFormat(format!(
                    "Failed to parse type argument: {}",
                    e
                ))
            })?,
        )],
        arguments: vec![
            serde_json::Value::String(owner.to_hex_literal()),
            serde_json::Value::String(asset.to_hex_literal()),
        ],
    };

    let response = provider
        .rest_client()
        .view(&view_request, None)
        .await
        .map_err(|e| {
            PaymentVerificationError::InvalidFormat(format!("Balance query failed: {}", e))
        })?;

    let values = response.into_inner();
    let balance_str = values
        .first()
        .and_then(|v| v.as_str())
        .ok_or_else(|| {
            PaymentVerificationError::InvalidFormat(
                "Unexpected balance response format".to_string(),
            )
        })?;

    balance_str.parse::<u64>().map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!("Failed to parse balance: {}", e))
    })
}

/// Simulate the transaction to verify it would succeed.
///
/// The Aptos simulate endpoint requires that signatures are NOT valid (as a security measure).
/// We use `NoAccountAuthenticator` for both sender and fee payer, which the node accepts
/// during simulation without checking on-chain auth keys.
async fn simulate_transaction(
    provider: &AptosChainProvider,
    deserialized: &DeserializedAptosTransaction,
) -> Result<(), PaymentVerificationError> {
    use aptos_types::transaction::authenticator::TransactionAuthenticator;

    let signed_txn = if let Some(fee_payer_address) = deserialized.fee_payer_address {
        // For sponsored transactions, use NoAccountAuthenticator for both sender and fee payer
        SignedTransaction::new_signed_transaction(
            deserialized.raw_transaction.clone(),
            TransactionAuthenticator::fee_payer(
                AccountAuthenticator::NoAccountAuthenticator,
                vec![],
                vec![],
                fee_payer_address,
                AccountAuthenticator::NoAccountAuthenticator,
            ),
        )
    } else {
        // For non-sponsored transactions, use SingleSender with NoAccountAuthenticator
        SignedTransaction::new_signed_transaction(
            deserialized.raw_transaction.clone(),
            TransactionAuthenticator::SingleSender {
                sender: AccountAuthenticator::NoAccountAuthenticator,
            },
        )
    };

    let result = provider
        .rest_client()
        .simulate(&signed_txn)
        .await
        .map_err(|e| {
            PaymentVerificationError::TransactionSimulation(format!(
                "Transaction simulation request failed: {}",
                e
            ))
        })?;

    let simulated = result.into_inner();
    let first = simulated.first().ok_or_else(|| {
        PaymentVerificationError::TransactionSimulation(
            "Empty simulation result".to_string(),
        )
    })?;

    if !first.info.success {
        return Err(PaymentVerificationError::TransactionSimulation(format!(
            "invalid_exact_aptos_payload_simulation_failed: {}",
            first.info.vm_status
        )));
    }

    Ok(())
}

/// Settle the transaction by submitting it to the network.
pub async fn settle_transaction(
    provider: &AptosChainProvider,
    verification: VerifyTransferResult,
) -> Result<String, PaymentVerificationError> {
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

    let signed_txn = if let Some(fee_payer_address) = verification.fee_payer_address {
        // Sponsored transaction: facilitator signs as fee payer
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
        // Non-sponsored transaction: client pays own gas
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

    // Compute transaction hash
    let tx_hash = signed_txn.committed_hash();

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

    // Wait for transaction confirmation.
    // Re-serialize RawTransaction to extract expiration_timestamp_secs (private field).
    let raw_tx_bytes = bcs::to_bytes(&verification.raw_transaction).map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!(
            "Failed to serialize RawTransaction: {}",
            e
        ))
    })?;
    let raw_fields: RawTransactionFields = bcs::from_bytes(&raw_tx_bytes).map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!(
            "Failed to deserialize RawTransaction fields: {}",
            e
        ))
    })?;

    provider
        .rest_client()
        .wait_for_transaction_by_hash(
            tx_hash,
            raw_fields.expiration_timestamp_secs,
            None,
            None,
        )
        .await
        .map_err(|e| {
            PaymentVerificationError::TransactionSimulation(format!(
                "Transaction confirmation failed: {}",
                e
            ))
        })?;

    Ok(format!("0x{}", hex::encode(tx_hash.to_vec())))
}

/// Try to parse transaction_bytes as RawTransaction + None suffix (1 byte),
/// or as a bare RawTransaction without any suffix.
fn try_none_suffix_or_bare(
    transaction_bytes: &[u8],
) -> Result<(RawTransaction, Option<AccountAddress>), PaymentVerificationError> {
    // Try with None suffix (last byte = 0x00)
    if transaction_bytes.len() > 1 {
        let split_none = transaction_bytes.len() - 1;
        if transaction_bytes[split_none] == 0x00 {
            if let Ok(raw_tx) =
                bcs::from_bytes::<RawTransaction>(&transaction_bytes[..split_none])
            {
                return Ok((raw_tx, None));
            }
        }
    }

    // Try bare (no suffix)
    let raw_tx: RawTransaction = bcs::from_bytes(transaction_bytes).map_err(|e| {
        PaymentVerificationError::InvalidFormat(format!(
            "Failed to deserialize RawTransaction: {}",
            e
        ))
    })?;
    Ok((raw_tx, None))
}

/// Deserialize Aptos transaction from base64-encoded JSON.
///
/// The payload is base64-encoded JSON with `transaction` (BCS bytes of SimpleTransaction)
/// and `senderAuthenticator` (BCS bytes of AccountAuthenticator).
///
/// A SimpleTransaction is `RawTransaction || Option<AccountAddress>` in BCS.
fn deserialize_aptos_transaction(
    transaction_b64: &str,
) -> Result<DeserializedAptosTransaction, PaymentVerificationError> {
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

    // Deserialize RawTransaction from BCS.
    // The transaction bytes represent a SimpleTransaction: RawTransaction || Option<AccountAddress>
    //
    // BCS's `from_bytes` requires all bytes to be consumed, so we must split the buffer.
    // Option<AccountAddress> in BCS is either:
    //   - 1 byte  [0x00] for None
    //   - 33 bytes [0x01 + 32-byte address] for Some
    //
    // Strategy: try Some(address) suffix first (33 bytes), then None suffix (1 byte),
    // then assume no suffix (raw transaction is the full buffer).
    let (raw_transaction, fee_payer_address) = if transaction_bytes.len() > 33 {
        // Try parsing with Some(fee_payer) suffix (33 bytes)
        let split_some = transaction_bytes.len() - 33;
        if transaction_bytes[split_some] == 0x01 {
            // Looks like Some variant — try deserializing raw tx from prefix
            match bcs::from_bytes::<RawTransaction>(&transaction_bytes[..split_some]) {
                Ok(raw_tx) => {
                    let suffix = &transaction_bytes[split_some..];
                    let opt_addr: Option<AccountAddress> =
                        bcs::from_bytes(suffix).map_err(|e| {
                            PaymentVerificationError::InvalidFormat(format!(
                                "Failed to deserialize fee payer address: {}",
                                e
                            ))
                        })?;
                    (raw_tx, opt_addr)
                }
                Err(_) => {
                    // Fall through to try None suffix
                    try_none_suffix_or_bare(&transaction_bytes)?
                }
            }
        } else {
            try_none_suffix_or_bare(&transaction_bytes)?
        }
    } else if transaction_bytes.len() > 1 {
        try_none_suffix_or_bare(&transaction_bytes)?
    } else {
        let raw_tx: RawTransaction =
            bcs::from_bytes(&transaction_bytes).map_err(|e| {
                PaymentVerificationError::InvalidFormat(format!(
                    "Failed to deserialize RawTransaction: {}",
                    e
                ))
            })?;
        (raw_tx, None)
    };

    // Clone raw_transaction before consuming it with into_payload
    let raw_transaction_clone = raw_transaction.clone();

    // Extract entry function from payload
    let entry_function = match raw_transaction.into_payload() {
        aptos_types::transaction::TransactionPayload::EntryFunction(ef) => ef,
        _ => {
            return Err(PaymentVerificationError::InvalidFormat(
                "Expected EntryFunction payload".to_string(),
            ));
        }
    };

    Ok(DeserializedAptosTransaction {
        raw_transaction: raw_transaction_clone,
        fee_payer_address,
        authenticator_bytes,
        entry_function,
    })
}

#[cfg(test)]
#[path = "facilitator_tests.rs"]
mod tests;
