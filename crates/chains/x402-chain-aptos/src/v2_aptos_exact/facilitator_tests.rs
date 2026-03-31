//! Tests for the V2 Aptos Exact facilitator.

use super::*;
use aptos_crypto::ed25519::Ed25519PrivateKey;
use aptos_crypto::{SigningKey, Uniform};
use aptos_types::chain_id::ChainId as AptosChainId;
use aptos_types::transaction::TransactionPayload;
use move_core_types::identifier::Identifier;
use move_core_types::language_storage::{ModuleId, StructTag, TypeTag};

// ──────────────────────────────────────────────────
// Test helpers
// ──────────────────────────────────────────────────

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
