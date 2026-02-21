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
mod tests {
    use super::*;
    use aptos_crypto::ed25519::Ed25519PrivateKey;
    use aptos_crypto::{SigningKey, Uniform};
    use aptos_types::chain_id::ChainId as AptosChainId;
    use aptos_types::transaction::TransactionPayload;
    use move_core_types::identifier::Identifier;
    use move_core_types::language_storage::{ModuleId, StructTag, TypeTag};

    /// Create a test transfer EntryFunction for `0x1::primary_fungible_store::transfer`.
    fn create_transfer_entry_function(
        asset: AccountAddress,
        recipient: AccountAddress,
        amount: u64,
    ) -> EntryFunction {
        let module = ModuleId::new(
            AccountAddress::ONE,
            Identifier::new("primary_fungible_store").unwrap(),
        );
        let function = Identifier::new("transfer").unwrap();
        let type_arg = TypeTag::Struct(Box::new(StructTag {
            address: AccountAddress::ONE,
            module: Identifier::new("fungible_asset").unwrap(),
            name: Identifier::new("Metadata").unwrap(),
            type_args: vec![],
        }));
        EntryFunction::new(
            module,
            function,
            vec![type_arg],
            vec![
                bcs::to_bytes(&asset).unwrap(),
                bcs::to_bytes(&recipient).unwrap(),
                bcs::to_bytes(&amount).unwrap(),
            ],
        )
    }

    /// Create a test RawTransaction.
    fn create_test_raw_transaction(
        sender: AccountAddress,
        entry_function: EntryFunction,
        max_gas_amount: u64,
        expiration_timestamp_secs: u64,
        chain_id: u8,
    ) -> RawTransaction {
        RawTransaction::new(
            sender,
            0, // sequence number
            TransactionPayload::EntryFunction(entry_function),
            max_gas_amount,
            100, // gas unit price
            expiration_timestamp_secs,
            AptosChainId::new(chain_id),
        )
    }

    /// Encode a SimpleTransaction (RawTransaction + optional fee payer) into the
    /// base64 JSON payload format expected by `deserialize_aptos_transaction`.
    fn encode_simple_transaction(
        raw_tx: &RawTransaction,
        fee_payer: Option<AccountAddress>,
        authenticator: &AccountAuthenticator,
    ) -> String {
        let mut tx_bytes = bcs::to_bytes(raw_tx).unwrap();
        // Append Option<AccountAddress> in BCS
        let opt_bytes = bcs::to_bytes(&fee_payer).unwrap();
        tx_bytes.extend_from_slice(&opt_bytes);

        let auth_bytes = bcs::to_bytes(authenticator).unwrap();

        let json_payload = serde_json::json!({
            "transaction": tx_bytes.iter().map(|b| *b as u64).collect::<Vec<u64>>(),
            "senderAuthenticator": auth_bytes.iter().map(|b| *b as u64).collect::<Vec<u64>>(),
        });
        let json_str = serde_json::to_string(&json_payload).unwrap();
        let b64_bytes = Base64Bytes::encode(json_str.as_bytes());
        std::str::from_utf8(b64_bytes.as_ref()).unwrap().to_string()
    }

    /// Helper to base64-encode arbitrary bytes (for error-path tests).
    fn b64_encode(data: &[u8]) -> String {
        let b64_bytes = Base64Bytes::encode(data);
        std::str::from_utf8(b64_bytes.as_ref()).unwrap().to_string()
    }

    /// Generate a random Ed25519 keypair and derive the account address.
    fn generate_test_keypair() -> (Ed25519PrivateKey, aptos_crypto::ed25519::Ed25519PublicKey, AccountAddress) {
        let private_key = Ed25519PrivateKey::generate_for_testing();
        let public_key: aptos_crypto::ed25519::Ed25519PublicKey = (&private_key).into();
        use aptos_types::transaction::authenticator::AuthenticationKey;
        let auth_key = AuthenticationKey::ed25519(&public_key);
        let address = auth_key.account_address();
        (private_key, public_key, address)
    }

    /// Sign a RawTransaction (non-fee-payer) and return the AccountAuthenticator.
    fn sign_raw_transaction(
        private_key: &Ed25519PrivateKey,
        public_key: &aptos_crypto::ed25519::Ed25519PublicKey,
        raw_tx: &RawTransaction,
    ) -> AccountAuthenticator {
        let signature = private_key.sign(raw_tx).unwrap();
        AccountAuthenticator::ed25519(public_key.clone(), signature)
    }

    /// Sign a fee-payer RawTransaction and return the sender's AccountAuthenticator.
    fn sign_fee_payer_transaction(
        private_key: &Ed25519PrivateKey,
        public_key: &aptos_crypto::ed25519::Ed25519PublicKey,
        raw_tx: &RawTransaction,
        fee_payer_address: AccountAddress,
    ) -> AccountAuthenticator {
        use aptos_types::transaction::RawTransactionWithData;
        let fee_payer_msg = RawTransactionWithData::new_fee_payer(
            raw_tx.clone(),
            vec![],
            fee_payer_address,
        );
        let signature = private_key.sign(&fee_payer_msg).unwrap();
        AccountAuthenticator::ed25519(public_key.clone(), signature)
    }

    // ──────────────────────────────────────────────────
    // Tests for RawTransactionFields mirror struct
    // ──────────────────────────────────────────────────

    #[test]
    fn test_raw_transaction_fields_roundtrip() {
        let sender = AccountAddress::random();
        let asset = AccountAddress::random();
        let recipient = AccountAddress::random();
        let ef = create_transfer_entry_function(asset, recipient, 1_000_000);
        let raw_tx = create_test_raw_transaction(sender, ef, 200_000, 9999999999, 2);

        let bytes = bcs::to_bytes(&raw_tx).unwrap();
        let fields: RawTransactionFields = bcs::from_bytes(&bytes).unwrap();

        assert_eq!(fields.sender, sender);
        assert_eq!(fields.max_gas_amount, 200_000);
        assert_eq!(fields.expiration_timestamp_secs, 9999999999);
        assert_eq!(fields.chain_id.id(), 2);
    }

    #[test]
    fn test_raw_transaction_fields_mainnet_chain_id() {
        let ef = create_transfer_entry_function(
            AccountAddress::ONE,
            AccountAddress::TWO,
            100,
        );
        let raw_tx = create_test_raw_transaction(AccountAddress::random(), ef, 100_000, 1000, 1);

        let bytes = bcs::to_bytes(&raw_tx).unwrap();
        let fields: RawTransactionFields = bcs::from_bytes(&bytes).unwrap();

        assert_eq!(fields.chain_id.id(), 1);
    }

    // ──────────────────────────────────────────────────
    // Tests for deserialize_aptos_transaction
    // ──────────────────────────────────────────────────

    #[test]
    fn test_deserialize_simple_transaction_no_fee_payer() {
        let (priv_key, pub_key, sender) = generate_test_keypair();
        let asset = AccountAddress::random();
        let recipient = AccountAddress::random();
        let amount: u64 = 500_000;

        let ef = create_transfer_entry_function(asset, recipient, amount);
        let raw_tx = create_test_raw_transaction(sender, ef, 200_000, 9999999999, 2);
        let authenticator = sign_raw_transaction(&priv_key, &pub_key, &raw_tx);

        let b64 = encode_simple_transaction(&raw_tx, None, &authenticator);

        let result = deserialize_aptos_transaction(&b64).unwrap();

        assert_eq!(result.raw_transaction.sender(), sender);
        assert!(result.fee_payer_address.is_none());

        // Verify entry function fields
        assert_eq!(
            *result.entry_function.module().address(),
            AccountAddress::ONE
        );
        assert_eq!(result.entry_function.module().name().to_string(), "primary_fungible_store");
        assert_eq!(result.entry_function.function().to_string(), "transfer");
        assert_eq!(result.entry_function.args().len(), 3);

        // Verify args
        let parsed_asset: AccountAddress = bcs::from_bytes(&result.entry_function.args()[0]).unwrap();
        let parsed_recipient: AccountAddress = bcs::from_bytes(&result.entry_function.args()[1]).unwrap();
        let parsed_amount: u64 = bcs::from_bytes(&result.entry_function.args()[2]).unwrap();
        assert_eq!(parsed_asset, asset);
        assert_eq!(parsed_recipient, recipient);
        assert_eq!(parsed_amount, amount);
    }

    #[test]
    fn test_deserialize_fee_payer_transaction() {
        let (priv_key, pub_key, sender) = generate_test_keypair();
        let fee_payer = AccountAddress::random();
        let asset = AccountAddress::random();
        let recipient = AccountAddress::random();
        let amount: u64 = 1_000_000;

        let ef = create_transfer_entry_function(asset, recipient, amount);
        let raw_tx = create_test_raw_transaction(sender, ef, 200_000, 9999999999, 2);
        let authenticator = sign_fee_payer_transaction(&priv_key, &pub_key, &raw_tx, fee_payer);

        let b64 = encode_simple_transaction(&raw_tx, Some(fee_payer), &authenticator);

        let result = deserialize_aptos_transaction(&b64).unwrap();

        assert_eq!(result.raw_transaction.sender(), sender);
        assert_eq!(result.fee_payer_address, Some(fee_payer));
    }

    #[test]
    fn test_deserialize_invalid_base64() {
        let result = deserialize_aptos_transaction("!!!not-base64!!!");
        assert!(result.is_err());
        match result.unwrap_err() {
            PaymentVerificationError::InvalidFormat(msg) => {
                assert!(msg.contains("Base64 decode failed") || msg.contains("JSON parse failed"),
                    "unexpected error: {}", msg);
            }
            e => panic!("Expected InvalidFormat, got: {:?}", e),
        }
    }

    #[test]
    fn test_deserialize_invalid_json() {
        let b64 = b64_encode(b"not valid json");
        let result = deserialize_aptos_transaction(&b64);
        assert!(result.is_err());
        match result.unwrap_err() {
            PaymentVerificationError::InvalidFormat(msg) => {
                assert!(msg.contains("JSON parse failed"), "unexpected error: {}", msg);
            }
            e => panic!("Expected InvalidFormat, got: {:?}", e),
        }
    }

    #[test]
    fn test_deserialize_missing_transaction_field() {
        let json = serde_json::json!({ "senderAuthenticator": [0, 1, 2] });
        let b64 = b64_encode(serde_json::to_string(&json).unwrap().as_bytes());
        let result = deserialize_aptos_transaction(&b64);
        assert!(result.is_err());
        match result.unwrap_err() {
            PaymentVerificationError::InvalidFormat(msg) => {
                assert!(msg.contains("Missing transaction field"), "unexpected: {}", msg);
            }
            e => panic!("Expected InvalidFormat, got: {:?}", e),
        }
    }

    #[test]
    fn test_deserialize_missing_authenticator_field() {
        let json = serde_json::json!({ "transaction": [0, 1, 2] });
        let b64 = b64_encode(serde_json::to_string(&json).unwrap().as_bytes());
        let result = deserialize_aptos_transaction(&b64);
        assert!(result.is_err());
        match result.unwrap_err() {
            PaymentVerificationError::InvalidFormat(msg) => {
                assert!(msg.contains("Missing senderAuthenticator"), "unexpected: {}", msg);
            }
            e => panic!("Expected InvalidFormat, got: {:?}", e),
        }
    }

    // ──────────────────────────────────────────────────
    // Tests for Ed25519 sender-authenticator verification
    // ──────────────────────────────────────────────────

    #[test]
    fn test_ed25519_authenticator_address_derivation() {
        let (_, pub_key, expected_address) = generate_test_keypair();

        use aptos_types::transaction::authenticator::AuthenticationKey;
        let auth_key = AuthenticationKey::ed25519(&pub_key);
        let derived = auth_key.account_address();

        assert_eq!(derived, expected_address);
    }

    // ──────────────────────────────────────────────────
    // Tests for types
    // ──────────────────────────────────────────────────

    #[test]
    fn test_aptos_payment_requirements_extra_serde_with_fee_payer() {
        use crate::v2_aptos_exact::types::AptosPaymentRequirementsExtra;
        let addr: Address = "0x0000000000000000000000000000000000000000000000000000000000000001"
            .parse()
            .unwrap();
        let extra = AptosPaymentRequirementsExtra {
            fee_payer: Some(addr),
        };
        let json = serde_json::to_value(&extra).unwrap();
        assert_eq!(
            json,
            serde_json::json!({ "feePayer": "0x1" })
        );

        // Roundtrip
        let deserialized: AptosPaymentRequirementsExtra = serde_json::from_value(json).unwrap();
        assert!(deserialized.fee_payer.is_some());
    }

    #[test]
    fn test_aptos_payment_requirements_extra_serde_without_fee_payer() {
        use crate::v2_aptos_exact::types::AptosPaymentRequirementsExtra;
        let extra = AptosPaymentRequirementsExtra { fee_payer: None };
        let json = serde_json::to_value(&extra).unwrap();
        // feePayer should be absent due to skip_serializing_if
        assert_eq!(json, serde_json::json!({}));

        // Deserialize from empty object
        let deserialized: AptosPaymentRequirementsExtra =
            serde_json::from_value(serde_json::json!({})).unwrap();
        assert!(deserialized.fee_payer.is_none());
    }

    #[test]
    fn test_aptos_payment_requirements_extra_deserialize_from_ts_format() {
        use crate::v2_aptos_exact::types::AptosPaymentRequirementsExtra;
        // TS sends: { feePayer: "0xabcdef..." }
        let json = serde_json::json!({
            "feePayer": "0x0000000000000000000000000000000000000000000000000000000000000042"
        });
        let extra: AptosPaymentRequirementsExtra = serde_json::from_value(json).unwrap();
        assert!(extra.fee_payer.is_some());
        let addr = extra.fee_payer.unwrap();
        // AccountAddress uses standard display: special addresses (0x0-0xf) are short,
        // all others use full 66-char hex. 0x42 is not special, so it's long form.
        assert_eq!(
            addr.to_string(),
            "0x0000000000000000000000000000000000000000000000000000000000000042"
        );
    }

    #[test]
    fn test_option_extra_none_deserialization() {
        // PaymentRequirements.extra is Option<AptosPaymentRequirementsExtra>
        // When absent from JSON, it should be None
        let val: Option<crate::v2_aptos_exact::types::AptosPaymentRequirementsExtra> =
            serde_json::from_value(serde_json::Value::Null).unwrap();
        assert!(val.is_none());
    }

    // ──────────────────────────────────────────────────
    // Tests for entry function validation
    // ──────────────────────────────────────────────────

    #[test]
    fn test_primary_fungible_store_transfer_function_detection() {
        let ef = create_transfer_entry_function(
            AccountAddress::ONE,
            AccountAddress::TWO,
            100,
        );

        let module_address = *ef.module().address();
        let module_name = ef.module().name().to_string();
        let function_name = ef.function().to_string();

        assert_eq!(module_address, AccountAddress::ONE);
        assert_eq!(module_name, "primary_fungible_store");
        assert_eq!(function_name, "transfer");
    }

    #[test]
    fn test_fungible_asset_transfer_function_detection() {
        let module = ModuleId::new(
            AccountAddress::ONE,
            Identifier::new("fungible_asset").unwrap(),
        );
        let function = Identifier::new("transfer").unwrap();
        let type_arg = TypeTag::Struct(Box::new(StructTag {
            address: AccountAddress::ONE,
            module: Identifier::new("fungible_asset").unwrap(),
            name: Identifier::new("Metadata").unwrap(),
            type_args: vec![],
        }));
        let ef = EntryFunction::new(
            module,
            function,
            vec![type_arg],
            vec![
                bcs::to_bytes(&AccountAddress::ONE).unwrap(),
                bcs::to_bytes(&AccountAddress::TWO).unwrap(),
                bcs::to_bytes(&100u64).unwrap(),
            ],
        );

        let module_address = *ef.module().address();
        let module_name = ef.module().name().to_string();
        let function_name = ef.function().to_string();

        let is_primary = module_address == AccountAddress::ONE
            && module_name == "primary_fungible_store"
            && function_name == "transfer";

        let is_fungible = module_address == AccountAddress::ONE
            && module_name == "fungible_asset"
            && function_name == "transfer";

        assert!(!is_primary);
        assert!(is_fungible);
        assert!(is_primary || is_fungible);
    }

    #[test]
    fn test_wrong_module_rejected() {
        let module = ModuleId::new(
            AccountAddress::ONE,
            Identifier::new("coin").unwrap(),
        );
        let function = Identifier::new("transfer").unwrap();
        let ef = EntryFunction::new(module, function, vec![], vec![]);

        let module_address = *ef.module().address();
        let module_name = ef.module().name().to_string();
        let function_name = ef.function().to_string();

        let is_primary = module_address == AccountAddress::ONE
            && module_name == "primary_fungible_store"
            && function_name == "transfer";
        let is_fungible = module_address == AccountAddress::ONE
            && module_name == "fungible_asset"
            && function_name == "transfer";

        assert!(!is_primary && !is_fungible);
    }

    // ──────────────────────────────────────────────────
    // Tests for amount/address BCS parsing
    // ──────────────────────────────────────────────────

    #[test]
    fn test_bcs_amount_roundtrip() {
        let amount: u64 = 1_000_000;
        let bytes = bcs::to_bytes(&amount).unwrap();
        let parsed: u64 = bcs::from_bytes(&bytes).unwrap();
        assert_eq!(parsed, amount);
    }

    #[test]
    fn test_bcs_address_roundtrip() {
        let addr = AccountAddress::random();
        let bytes = bcs::to_bytes(&addr).unwrap();
        let parsed: AccountAddress = bcs::from_bytes(&bytes).unwrap();
        assert_eq!(parsed, addr);
    }

    // ──────────────────────────────────────────────────
    // Tests for fee payer serialization in SimpleTransaction
    // ──────────────────────────────────────────────────

    #[test]
    fn test_simple_transaction_fee_payer_extraction() {
        let sender = AccountAddress::random();
        let fee_payer = AccountAddress::random();
        let ef = create_transfer_entry_function(
            AccountAddress::ONE,
            AccountAddress::TWO,
            500,
        );
        let raw_tx = create_test_raw_transaction(sender, ef, 200_000, 9999999999, 2);

        // Serialize RawTransaction + Some(fee_payer) to mimic SimpleTransaction
        let mut tx_bytes = bcs::to_bytes(&raw_tx).unwrap();
        let opt_bytes = bcs::to_bytes(&Some(fee_payer)).unwrap();
        tx_bytes.extend_from_slice(&opt_bytes);

        // Now parse the way our deserializer does
        let raw_tx_reserialized = bcs::to_bytes(&raw_tx).unwrap();
        assert!(tx_bytes.len() > raw_tx_reserialized.len());

        let suffix = &tx_bytes[raw_tx_reserialized.len()..];
        let opt_addr: Option<AccountAddress> = bcs::from_bytes(suffix).unwrap();
        assert_eq!(opt_addr, Some(fee_payer));
    }

    #[test]
    fn test_simple_transaction_no_fee_payer_extraction() {
        let sender = AccountAddress::random();
        let ef = create_transfer_entry_function(
            AccountAddress::ONE,
            AccountAddress::TWO,
            500,
        );
        let raw_tx = create_test_raw_transaction(sender, ef, 200_000, 9999999999, 2);

        // Serialize RawTransaction + None to mimic SimpleTransaction
        let mut tx_bytes = bcs::to_bytes(&raw_tx).unwrap();
        let opt_bytes = bcs::to_bytes(&None::<AccountAddress>).unwrap();
        tx_bytes.extend_from_slice(&opt_bytes);

        let raw_tx_reserialized = bcs::to_bytes(&raw_tx).unwrap();
        let suffix = &tx_bytes[raw_tx_reserialized.len()..];
        let opt_addr: Option<AccountAddress> = bcs::from_bytes(suffix).unwrap();
        assert_eq!(opt_addr, None);
    }

    // ──────────────────────────────────────────────────
    // Tests for expiration check logic
    // ──────────────────────────────────────────────────

    #[test]
    fn test_expiration_check_future_ok() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expiration = now + 60; // 60 seconds from now
        assert!(expiration >= now + EXPIRATION_BUFFER_SECONDS);
    }

    #[test]
    fn test_expiration_check_too_close_fails() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expiration = now + 3; // Only 3 seconds buffer, needs 5
        assert!(expiration < now + EXPIRATION_BUFFER_SECONDS);
    }

    #[test]
    fn test_expiration_check_past_fails() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let expiration = now - 10; // 10 seconds ago
        assert!(expiration < now + EXPIRATION_BUFFER_SECONDS);
    }

    // ──────────────────────────────────────────────────
    // Tests for max gas amount check
    // ──────────────────────────────────────────────────

    #[test]
    fn test_max_gas_within_limit() {
        assert!(200_000u64 <= MAX_GAS_AMOUNT);
        assert!(500_000u64 <= MAX_GAS_AMOUNT);
    }

    #[test]
    fn test_max_gas_exceeds_limit() {
        assert!(500_001u64 > MAX_GAS_AMOUNT);
        assert!(1_000_000u64 > MAX_GAS_AMOUNT);
    }

    // ──────────────────────────────────────────────────
    // End-to-end deserialization + field validation test
    // ──────────────────────────────────────────────────

    #[test]
    fn test_full_deserialization_and_field_validation() {
        let (priv_key, pub_key, sender) = generate_test_keypair();
        let fee_payer = AccountAddress::random();
        let asset = AccountAddress::random();
        let recipient = AccountAddress::random();
        let amount: u64 = 42_000_000;

        let ef = create_transfer_entry_function(asset, recipient, amount);
        let raw_tx = create_test_raw_transaction(sender, ef, 300_000, 9999999999, 2);
        let authenticator = sign_fee_payer_transaction(&priv_key, &pub_key, &raw_tx, fee_payer);

        let b64 = encode_simple_transaction(&raw_tx, Some(fee_payer), &authenticator);

        // Deserialize
        let deserialized = deserialize_aptos_transaction(&b64).unwrap();
        assert_eq!(deserialized.raw_transaction.sender(), sender);
        assert_eq!(deserialized.fee_payer_address, Some(fee_payer));

        // Verify fields via mirror struct
        let raw_bytes = bcs::to_bytes(&deserialized.raw_transaction).unwrap();
        let fields: RawTransactionFields = bcs::from_bytes(&raw_bytes).unwrap();
        assert_eq!(fields.sender, sender);
        assert_eq!(fields.max_gas_amount, 300_000);
        assert_eq!(fields.expiration_timestamp_secs, 9999999999);
        assert_eq!(fields.chain_id.id(), 2);

        // Verify entry function
        let ef = &deserialized.entry_function;
        assert_eq!(*ef.module().address(), AccountAddress::ONE);
        assert_eq!(ef.module().name().to_string(), "primary_fungible_store");
        assert_eq!(ef.function().to_string(), "transfer");
        assert_eq!(ef.ty_args().len(), 1);
        assert_eq!(ef.args().len(), 3);

        // Verify args
        let parsed_asset: AccountAddress = bcs::from_bytes(&ef.args()[0]).unwrap();
        let parsed_recipient: AccountAddress = bcs::from_bytes(&ef.args()[1]).unwrap();
        let parsed_amount: u64 = bcs::from_bytes(&ef.args()[2]).unwrap();
        assert_eq!(parsed_asset, asset);
        assert_eq!(parsed_recipient, recipient);
        assert_eq!(parsed_amount, amount);

        // Verify authenticator can be deserialized back
        let sender_auth: AccountAuthenticator =
            bcs::from_bytes(&deserialized.authenticator_bytes).unwrap();
        if let AccountAuthenticator::Ed25519 {
            public_key: pk, ..
        } = &sender_auth
        {
            use aptos_types::transaction::authenticator::AuthenticationKey;
            let derived = AuthenticationKey::ed25519(pk).account_address();
            assert_eq!(derived, sender);
        } else {
            panic!("Expected Ed25519 authenticator");
        }
    }

    // ──────────────────────────────────────────────────
    // Tests for supported() response format
    // ──────────────────────────────────────────────────

    #[test]
    fn test_supported_extra_format() {
        // Simulate what supported() produces
        let fee_payer_address = AccountAddress::from_hex_literal(
            "0x0000000000000000000000000000000000000000000000000000000000000042",
        )
        .unwrap();
        let extra = serde_json::json!({ "feePayer": Address::new(fee_payer_address).to_string() });

        // Should have feePayer key (camelCase)
        assert!(extra.get("feePayer").is_some());
        // AccountAddress standard display uses long form for non-special addresses
        assert_eq!(
            extra["feePayer"],
            "0x0000000000000000000000000000000000000000000000000000000000000042"
        );

        // Should NOT have "sponsored" key
        assert!(extra.get("sponsored").is_none());
    }

    // ──────────────────────────────────────────────────
    // Tests for USDC testnet address
    // ──────────────────────────────────────────────────

    #[test]
    fn test_usdc_testnet_address() {
        use crate::networks::KnownNetworkAptos;
        use x402_types::networks::USDC;

        let deployment = USDC::aptos_testnet();
        assert_eq!(
            deployment.address.to_string(),
            "0x69091fbab5f7d635ee7ac5098cf0c1efbe31d68fec0f2cd565e8d168daf52832"
        );
    }

    #[test]
    fn test_usdc_mainnet_address() {
        use crate::networks::KnownNetworkAptos;
        use x402_types::networks::USDC;

        let deployment = USDC::aptos();
        assert_eq!(
            deployment.address.to_string(),
            "0xbae207659db88bea0cbead6da0ed00aac12edcdda169e591cd41c94180b46f3b"
        );
    }
}
