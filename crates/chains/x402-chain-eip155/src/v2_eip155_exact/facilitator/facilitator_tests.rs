//! Tests for the V2 EIP-155 Exact facilitator.
//!
//! These tests cover offline/pure validation functions that do not require
//! on-chain calls, following the same pattern as the Aptos facilitator tests.

use alloy_primitives::{Address, Bytes, U256, address};
use x402_types::proto::PaymentVerificationError;
use x402_types::proto::v2::{self, X402Version2};
use x402_types::timestamp::UnixTimestamp;

use crate::chain::permit2::{
    EXACT_PERMIT2_PROXY_ADDRESS, ExactPermit2Witness, Permit2Authorization,
    Permit2AuthorizationPermitted,
};
use crate::chain::{AssetTransferMethod, ChecksummedAddress};
use crate::v1_eip155_exact::facilitator::{assert_enough_value, assert_time};
use crate::v2_eip155_exact::facilitator::eip3009::assert_requirements_match;
use crate::v2_eip155_exact::facilitator::permit2::assert_offchain_valid;
use crate::v2_eip155_exact::types::{
    Eip3009PaymentRequirements, FacilitatorVerifyRequest, Permit2PaymentPayload,
    Permit2PaymentRequirements,
};

// ──────────────────────────────────────────────────
// Test helpers
// ──────────────────────────────────────────────────

const TEST_PAYER: Address = address!("0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
const TEST_RECIPIENT: Address = address!("0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");
const TEST_TOKEN: Address = address!("0x833589fCD6eDb6E08f4c7C32D4f71b54bdA02913"); // USDC on Base

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn make_eip3009_requirements(
    amount: U256,
    pay_to: Address,
) -> Eip3009PaymentRequirements {
    serde_json::from_value(serde_json::json!({
        "scheme": "exact",
        "network": "eip155:8453",
        "amount": amount.to_string(),
        "asset": ChecksummedAddress(TEST_TOKEN).to_string(),
        "payTo": ChecksummedAddress(pay_to).to_string(),
        "maxTimeoutSeconds": 300,
        "extra": {
            "assetTransferMethod": "eip3009",
            "name": "USD Coin",
            "version": "2"
        }
    }))
    .expect("valid Eip3009PaymentRequirements JSON")
}

fn make_permit2_requirements(
    amount: U256,
    pay_to: Address,
) -> Permit2PaymentRequirements {
    serde_json::from_value(serde_json::json!({
        "scheme": "exact",
        "network": "eip155:8453",
        "amount": amount.to_string(),
        "asset": ChecksummedAddress(TEST_TOKEN).to_string(),
        "payTo": ChecksummedAddress(pay_to).to_string(),
        "maxTimeoutSeconds": 300,
        "extra": {
            "assetTransferMethod": "permit2"
        }
    }))
    .expect("valid Permit2PaymentRequirements JSON")
}

fn make_permit2_payload(
    requirements: &Permit2PaymentRequirements,
    spender: Address,
    recipient: Address,
    amount: U256,
    token: Address,
    valid_after: u64,
    deadline: u64,
) -> Permit2PaymentPayload {
    let payload = crate::chain::permit2::ExactPermit2Payload {
        permit_2_authorization: Permit2Authorization {
            deadline: UnixTimestamp::from_secs(deadline),
            from: ChecksummedAddress(TEST_PAYER),
            nonce: U256::from(1u64),
            permitted: Permit2AuthorizationPermitted {
                amount,
                token: ChecksummedAddress(token),
            },
            spender: ChecksummedAddress(spender),
            witness: ExactPermit2Witness {
                to: ChecksummedAddress(recipient),
                valid_after: UnixTimestamp::from_secs(valid_after),
            },
        },
        signature: Bytes::from(vec![0u8; 65]),
    };
    v2::PaymentPayload {
        accepted: requirements.clone(),
        payload,
        resource: None,
        x402_version: X402Version2,
        extensions: None,
    }
}

// ──────────────────────────────────────────────────
// assert_requirements_match
// ──────────────────────────────────────────────────

#[test]
fn requirements_match_identical() {
    let req = make_eip3009_requirements(U256::from(1_000_000u64), TEST_RECIPIENT);
    assert!(assert_requirements_match(&req, &req).is_ok());
}

#[test]
fn requirements_match_different_amount() {
    let req_a = make_eip3009_requirements(U256::from(1_000_000u64), TEST_RECIPIENT);
    let req_b = make_eip3009_requirements(U256::from(2_000_000u64), TEST_RECIPIENT);
    let err = assert_requirements_match(&req_a, &req_b).unwrap_err();
    assert!(matches!(err, PaymentVerificationError::AcceptedRequirementsMismatch));
}

#[test]
fn requirements_match_different_recipient() {
    let other = address!("0xcccccccccccccccccccccccccccccccccccccccc");
    let req_a = make_eip3009_requirements(U256::from(1_000_000u64), TEST_RECIPIENT);
    let req_b = make_eip3009_requirements(U256::from(1_000_000u64), other);
    let err = assert_requirements_match(&req_a, &req_b).unwrap_err();
    assert!(matches!(err, PaymentVerificationError::AcceptedRequirementsMismatch));
}

// ──────────────────────────────────────────────────
// assert_enough_value
// ──────────────────────────────────────────────────

#[test]
fn enough_value_exact() {
    let sent = U256::from(1_000_000u64);
    let required = U256::from(1_000_000u64);
    assert!(assert_enough_value(&sent, &required).is_ok());
}

#[test]
fn enough_value_overpay() {
    let sent = U256::from(2_000_000u64);
    let required = U256::from(1_000_000u64);
    assert!(assert_enough_value(&sent, &required).is_ok());
}

#[test]
fn enough_value_underpay() {
    let sent = U256::from(500_000u64);
    let required = U256::from(1_000_000u64);
    let err = assert_enough_value(&sent, &required).unwrap_err();
    assert!(matches!(err, PaymentVerificationError::InvalidPaymentAmount));
}

#[test]
fn enough_value_zero() {
    let sent = U256::ZERO;
    let required = U256::from(1u64);
    let err = assert_enough_value(&sent, &required).unwrap_err();
    assert!(matches!(err, PaymentVerificationError::InvalidPaymentAmount));
}

// ──────────────────────────────────────────────────
// assert_time
// ──────────────────────────────────────────────────

#[test]
fn time_valid_window() {
    let now = now_secs();
    let valid_after = UnixTimestamp::from_secs(now - 60);
    let valid_before = UnixTimestamp::from_secs(now + 300);
    assert!(assert_time(valid_after, valid_before).is_ok());
}

#[test]
fn time_expired() {
    let now = now_secs();
    // valid_before in the past (must be > now + 6)
    let valid_after = UnixTimestamp::from_secs(now - 120);
    let valid_before = UnixTimestamp::from_secs(now - 10);
    let err = assert_time(valid_after, valid_before).unwrap_err();
    assert!(matches!(err, PaymentVerificationError::Expired));
}

#[test]
fn time_not_yet_valid() {
    let now = now_secs();
    let valid_after = UnixTimestamp::from_secs(now + 300);
    let valid_before = UnixTimestamp::from_secs(now + 600);
    let err = assert_time(valid_after, valid_before).unwrap_err();
    assert!(matches!(err, PaymentVerificationError::Early));
}

#[test]
fn time_almost_expired() {
    let now = now_secs();
    // valid_before is now + 3, but threshold is now + 6
    let valid_after = UnixTimestamp::from_secs(now - 60);
    let valid_before = UnixTimestamp::from_secs(now + 3);
    let err = assert_time(valid_after, valid_before).unwrap_err();
    assert!(matches!(err, PaymentVerificationError::Expired));
}

// ──────────────────────────────────────────────────
// assert_offchain_valid (Permit2)
// ──────────────────────────────────────────────────

#[test]
fn offchain_valid_happy_path() {
    let now = now_secs();
    let amount = U256::from(1_000_000u64);
    let req = make_permit2_requirements(amount, TEST_RECIPIENT);
    let payload = make_permit2_payload(
        &req,
        EXACT_PERMIT2_PROXY_ADDRESS,
        TEST_RECIPIENT,
        amount,
        TEST_TOKEN,
        now - 60,
        now + 300,
    );
    assert!(assert_offchain_valid(&payload, &req).is_ok());
}

#[test]
fn offchain_valid_wrong_spender() {
    let now = now_secs();
    let amount = U256::from(1_000_000u64);
    let req = make_permit2_requirements(amount, TEST_RECIPIENT);
    let wrong_spender = address!("0xdeaddeaddeaddeaddeaddeaddeaddeaddeaddead");
    let payload = make_permit2_payload(
        &req,
        wrong_spender,
        TEST_RECIPIENT,
        amount,
        TEST_TOKEN,
        now - 60,
        now + 300,
    );
    let err = assert_offchain_valid(&payload, &req).unwrap_err();
    assert!(matches!(err, PaymentVerificationError::RecipientMismatch));
}

#[test]
fn offchain_valid_wrong_recipient() {
    let now = now_secs();
    let amount = U256::from(1_000_000u64);
    let req = make_permit2_requirements(amount, TEST_RECIPIENT);
    let wrong_recipient = address!("0xdeaddeaddeaddeaddeaddeaddeaddeaddeaddead");
    let payload = make_permit2_payload(
        &req,
        EXACT_PERMIT2_PROXY_ADDRESS,
        wrong_recipient,
        amount,
        TEST_TOKEN,
        now - 60,
        now + 300,
    );
    let err = assert_offchain_valid(&payload, &req).unwrap_err();
    assert!(matches!(err, PaymentVerificationError::RecipientMismatch));
}

#[test]
fn offchain_valid_insufficient_amount() {
    let now = now_secs();
    let required = U256::from(1_000_000u64);
    let offered = U256::from(500_000u64);
    let req = make_permit2_requirements(required, TEST_RECIPIENT);
    let payload = make_permit2_payload(
        &req,
        EXACT_PERMIT2_PROXY_ADDRESS,
        TEST_RECIPIENT,
        offered,
        TEST_TOKEN,
        now - 60,
        now + 300,
    );
    let err = assert_offchain_valid(&payload, &req).unwrap_err();
    assert!(matches!(err, PaymentVerificationError::InvalidPaymentAmount));
}

#[test]
fn offchain_valid_wrong_token() {
    let now = now_secs();
    let amount = U256::from(1_000_000u64);
    let req = make_permit2_requirements(amount, TEST_RECIPIENT);
    let wrong_token = address!("0x1111111111111111111111111111111111111111");
    let payload = make_permit2_payload(
        &req,
        EXACT_PERMIT2_PROXY_ADDRESS,
        TEST_RECIPIENT,
        amount,
        wrong_token,
        now - 60,
        now + 300,
    );
    let err = assert_offchain_valid(&payload, &req).unwrap_err();
    assert!(matches!(err, PaymentVerificationError::AssetMismatch));
}

#[test]
fn offchain_valid_expired() {
    let now = now_secs();
    let amount = U256::from(1_000_000u64);
    let req = make_permit2_requirements(amount, TEST_RECIPIENT);
    let payload = make_permit2_payload(
        &req,
        EXACT_PERMIT2_PROXY_ADDRESS,
        TEST_RECIPIENT,
        amount,
        TEST_TOKEN,
        now - 120,
        now - 10, // deadline in the past
    );
    let err = assert_offchain_valid(&payload, &req).unwrap_err();
    assert!(matches!(err, PaymentVerificationError::Expired));
}

#[test]
fn offchain_valid_not_yet_valid() {
    let now = now_secs();
    let amount = U256::from(1_000_000u64);
    let req = make_permit2_requirements(amount, TEST_RECIPIENT);
    let payload = make_permit2_payload(
        &req,
        EXACT_PERMIT2_PROXY_ADDRESS,
        TEST_RECIPIENT,
        amount,
        TEST_TOKEN,
        now + 300, // valid_after in the future
        now + 600,
    );
    let err = assert_offchain_valid(&payload, &req).unwrap_err();
    assert!(matches!(err, PaymentVerificationError::Early));
}

// ──────────────────────────────────────────────────
// FacilitatorVerifyRequest serde
// ──────────────────────────────────────────────────

#[test]
fn serde_facilitator_verify_request_eip3009() {
    let now = now_secs();
    let eip3009_reqs = serde_json::json!({
        "scheme": "exact",
        "network": "eip155:8453",
        "amount": "1000000",
        "asset": ChecksummedAddress(TEST_TOKEN).to_string(),
        "payTo": ChecksummedAddress(TEST_RECIPIENT).to_string(),
        "maxTimeoutSeconds": 300,
        "extra": {
            "assetTransferMethod": "eip3009",
            "name": "USD Coin",
            "version": "2"
        }
    });
    let json = serde_json::json!({
        "x402Version": 2,
        "paymentPayload": {
            "x402Version": 2,
            "accepted": eip3009_reqs,
            "payload": {
                "signature": "0x0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000ff",
                "authorization": {
                    "from": ChecksummedAddress(TEST_PAYER).to_string(),
                    "to": ChecksummedAddress(TEST_RECIPIENT).to_string(),
                    "value": "1000000",
                    "validAfter": (now - 60).to_string(),
                    "validBefore": (now + 300).to_string(),
                    "nonce": "0x0000000000000000000000000000000000000000000000000000000000000001"
                }
            }
        },
        "paymentRequirements": eip3009_reqs
    });
    let parsed: FacilitatorVerifyRequest =
        serde_json::from_value(json).expect("should deserialize Eip3009 variant");
    assert!(matches!(parsed, FacilitatorVerifyRequest::Eip3009 { .. }));

    // Roundtrip
    let serialized = serde_json::to_value(&parsed).expect("should serialize");
    let _reparsed: FacilitatorVerifyRequest =
        serde_json::from_value(serialized).expect("should roundtrip");
}

#[test]
fn serde_facilitator_verify_request_permit2() {
    let now = now_secs();
    let permit2_reqs = serde_json::json!({
        "scheme": "exact",
        "network": "eip155:8453",
        "amount": "1000000",
        "asset": ChecksummedAddress(TEST_TOKEN).to_string(),
        "payTo": ChecksummedAddress(TEST_RECIPIENT).to_string(),
        "maxTimeoutSeconds": 300,
        "extra": {
            "assetTransferMethod": "permit2"
        }
    });
    let json = serde_json::json!({
        "x402Version": 2,
        "paymentPayload": {
            "x402Version": 2,
            "accepted": permit2_reqs,
            "payload": {
                "permit2Authorization": {
                    "deadline": (now + 300).to_string(),
                    "from": ChecksummedAddress(TEST_PAYER).to_string(),
                    "nonce": "1",
                    "permitted": {
                        "amount": "1000000",
                        "token": ChecksummedAddress(TEST_TOKEN).to_string()
                    },
                    "spender": ChecksummedAddress(EXACT_PERMIT2_PROXY_ADDRESS).to_string(),
                    "witness": {
                        "to": ChecksummedAddress(TEST_RECIPIENT).to_string(),
                        "validAfter": (now - 60).to_string()
                    }
                },
                "signature": "0x0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000ff"
            }
        },
        "paymentRequirements": permit2_reqs
    });
    let parsed: FacilitatorVerifyRequest =
        serde_json::from_value(json).expect("should deserialize Permit2 variant");
    assert!(matches!(parsed, FacilitatorVerifyRequest::Permit2 { .. }));

    // Roundtrip
    let serialized = serde_json::to_value(&parsed).expect("should serialize");
    let _reparsed: FacilitatorVerifyRequest =
        serde_json::from_value(serialized).expect("should roundtrip");
}

#[test]
fn serde_facilitator_verify_request_invalid_json() {
    let json = serde_json::json!({
        "x402Version": 2,
        "paymentPayload": "not an object",
        "paymentRequirements": {}
    });
    let result = serde_json::from_value::<FacilitatorVerifyRequest>(json);
    assert!(result.is_err());
}

// ──────────────────────────────────────────────────
// ChecksummedAddress serde
// ──────────────────────────────────────────────────

#[test]
fn checksummed_address_roundtrip() {
    let addr = ChecksummedAddress(TEST_TOKEN);
    let serialized = serde_json::to_string(&addr).expect("should serialize");
    let deserialized: ChecksummedAddress =
        serde_json::from_str(&serialized).expect("should deserialize");
    assert_eq!(addr.0, deserialized.0);
}

#[test]
fn checksummed_address_display_is_eip55() {
    let addr = ChecksummedAddress(TEST_TOKEN);
    let display = addr.to_string();
    // EIP-55 checksummed addresses have mixed case
    assert!(display.starts_with("0x"));
    assert_eq!(display.len(), 42);
}

// ──────────────────────────────────────────────────
// AssetTransferMethod serde
// ──────────────────────────────────────────────────

#[test]
fn asset_transfer_method_eip3009_roundtrip() {
    let method = AssetTransferMethod::Eip3009 {
        name: "USD Coin".to_string(),
        version: "2".to_string(),
    };
    let json = serde_json::to_value(&method).expect("should serialize");
    let deserialized: AssetTransferMethod =
        serde_json::from_value(json).expect("should deserialize");
    assert!(matches!(deserialized, AssetTransferMethod::Eip3009 { .. }));
}

#[test]
fn asset_transfer_method_permit2_roundtrip() {
    let method = AssetTransferMethod::Permit2;
    let json = serde_json::to_value(&method).expect("should serialize");
    let deserialized: AssetTransferMethod =
        serde_json::from_value(json).expect("should deserialize");
    assert!(matches!(deserialized, AssetTransferMethod::Permit2));
}
