//! Type definitions for the V2 EIP-155 "upto" payment scheme.
//!
//! This module defines the wire format types for EIP-2612 permit-based batched payments
//! on EVM chains. Unlike the "exact" scheme which uses ERC-3009 for immediate settlement,
//! the "upto" scheme uses EIP-2612 permits to authorize a spending cap, enabling multiple
//! payments to be batched and settled together.

use alloy_primitives::{Address, Bytes, U256};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::chain::eip155::{ChecksummedAddress, TokenAmount};
use crate::lit_str;
use crate::proto::v2;

lit_str!(UptoScheme, "upto");

pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;
pub type SettleRequest = VerifyRequest;
pub type PaymentPayload = v2::PaymentPayload<PaymentRequirements, UptoEvmPayload>;
pub type PaymentRequirements =
    v2::PaymentRequirements<UptoScheme, TokenAmount, ChecksummedAddress, PaymentRequirementsExtra>;

/// Full payload required to authorize an EIP-2612 permit:
/// includes the signature and the authorization struct.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UptoEvmPayload {
    pub signature: Bytes,
    pub authorization: UptoEvmAuthorization,
}

/// EIP-2612 permit authorization data.
/// Defines who can spend how much tokens and when.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UptoEvmAuthorization {
    /// Token owner (the payer)
    pub from: Address,
    /// Spender (the facilitator)
    pub to: Address,
    /// Maximum spending cap
    pub value: U256,
    /// EIP-2612 nonce (from the token contract)
    pub nonce: U256,
    /// Deadline timestamp (seconds since epoch)
    pub valid_before: U256,
}

/// Extra requirements for upto payments.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentRequirementsExtra {
    /// EIP-712 domain name (e.g., "USD Coin")
    pub name: String,
    /// EIP-712 domain version (e.g., "2")
    pub version: String,
    /// Optional maximum cap requirement
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_amount_required: Option<U256>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{address, hex};

    // Test constants
    const TEST_OWNER: Address = address!("0x1111111111111111111111111111111111111111");
    const TEST_SPENDER: Address = address!("0x2222222222222222222222222222222222222222");
    const TEST_SIGNATURE: &str = "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12";

    #[test]
    fn test_upto_authorization_serialization() {
        let auth = UptoEvmAuthorization {
            from: TEST_OWNER,
            to: TEST_SPENDER,
            value: U256::from(1_000_000u64),
            nonce: U256::from(42u64),
            valid_before: U256::from(1_700_000_000u64),
        };

        let json = serde_json::to_string(&auth).unwrap();
        assert!(json.contains("\"from\""));
        assert!(json.contains("\"to\""));
        assert!(json.contains("\"value\""));
        assert!(json.contains("\"nonce\""));
        assert!(json.contains("\"validBefore\"")); // camelCase

        let deserialized: UptoEvmAuthorization = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.from, TEST_OWNER);
        assert_eq!(deserialized.to, TEST_SPENDER);
        assert_eq!(deserialized.value, U256::from(1_000_000u64));
        assert_eq!(deserialized.nonce, U256::from(42u64));
        assert_eq!(deserialized.valid_before, U256::from(1_700_000_000u64));
    }

    #[test]
    fn test_upto_payload_serialization() {
        let auth = UptoEvmAuthorization {
            from: TEST_OWNER,
            to: TEST_SPENDER,
            value: U256::from(1_000_000u64),
            nonce: U256::from(0u64),
            valid_before: U256::from(1_700_000_000u64),
        };

        let signature = Bytes::from(hex::decode(TEST_SIGNATURE.strip_prefix("0x").unwrap()).unwrap());
        let payload = UptoEvmPayload {
            signature: signature.clone(),
            authorization: auth,
        };

        let json = serde_json::to_string(&payload).unwrap();
        assert!(json.contains("\"signature\""));
        assert!(json.contains("\"authorization\""));

        let deserialized: UptoEvmPayload = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.authorization.from, TEST_OWNER);
        assert_eq!(deserialized.authorization.to, TEST_SPENDER);
        assert_eq!(deserialized.signature, signature);
    }

    #[test]
    fn test_payment_requirements_extra_serialization() {
        let extra = PaymentRequirementsExtra {
            name: "USD Coin".to_string(),
            version: "2".to_string(),
            max_amount_required: Some(U256::from(10_000_000u64)),
        };

        let json = serde_json::to_string(&extra).unwrap();
        assert!(json.contains("\"name\""));
        assert!(json.contains("\"version\""));
        assert!(json.contains("\"maxAmountRequired\"")); // camelCase

        let deserialized: PaymentRequirementsExtra = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.name, "USD Coin");
        assert_eq!(deserialized.version, "2");
        assert_eq!(deserialized.max_amount_required, Some(U256::from(10_000_000u64)));
    }

    #[test]
    fn test_payment_requirements_extra_optional_field() {
        // Test with max_amount_required
        let extra_with_max = PaymentRequirementsExtra {
            name: "Token".to_string(),
            version: "1".to_string(),
            max_amount_required: Some(U256::from(5_000_000u64)),
        };
        let json_with = serde_json::to_string(&extra_with_max).unwrap();
        assert!(json_with.contains("maxAmountRequired"));

        // Test without max_amount_required (should be omitted in serialization)
        let extra_without_max = PaymentRequirementsExtra {
            name: "Token".to_string(),
            version: "1".to_string(),
            max_amount_required: None,
        };
        let json_without = serde_json::to_string(&extra_without_max).unwrap();
        assert!(!json_without.contains("maxAmountRequired"));

        // Deserialization should work with or without the field
        let deserialized_with: PaymentRequirementsExtra = serde_json::from_str(&json_with).unwrap();
        assert_eq!(deserialized_with.max_amount_required, Some(U256::from(5_000_000u64)));

        let deserialized_without: PaymentRequirementsExtra = serde_json::from_str(&json_without).unwrap();
        assert_eq!(deserialized_without.max_amount_required, None);
    }

    #[test]
    fn test_upto_authorization_roundtrip() {
        let original = UptoEvmAuthorization {
            from: TEST_OWNER,
            to: TEST_SPENDER,
            value: U256::from(999_999_999_000_000u64),
            nonce: U256::from(u64::MAX),
            valid_before: U256::from(u64::MAX),
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: UptoEvmAuthorization = serde_json::from_str(&json).unwrap();

        assert_eq!(original.from, deserialized.from);
        assert_eq!(original.to, deserialized.to);
        assert_eq!(original.value, deserialized.value);
        assert_eq!(original.nonce, deserialized.nonce);
        assert_eq!(original.valid_before, deserialized.valid_before);
    }

    #[test]
    fn test_upto_payload_roundtrip() {
        let auth = UptoEvmAuthorization {
            from: TEST_OWNER,
            to: TEST_SPENDER,
            value: U256::from(1_000_000u64),
            nonce: U256::from(0u64),
            valid_before: U256::from(1_700_000_000u64),
        };

        let signature_bytes = hex::decode("1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12").unwrap();
        let signature = Bytes::from(signature_bytes.clone());

        let original = UptoEvmPayload {
            signature: signature.clone(),
            authorization: auth,
        };

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: UptoEvmPayload = serde_json::from_str(&json).unwrap();

        assert_eq!(original.authorization.from, deserialized.authorization.from);
        assert_eq!(original.authorization.to, deserialized.authorization.to);
        assert_eq!(original.authorization.value, deserialized.authorization.value);
        assert_eq!(original.signature, deserialized.signature);
    }

    #[test]
    fn test_camel_case_conversion() {
        let auth = UptoEvmAuthorization {
            from: TEST_OWNER,
            to: TEST_SPENDER,
            value: U256::from(1u64),
            nonce: U256::from(0u64),
            valid_before: U256::from(1u64),
        };

        let json = serde_json::to_string(&auth).unwrap();
        // Verify camelCase is used, not snake_case
        assert!(json.contains("\"validBefore\""));
        assert!(!json.contains("\"valid_before\""));
    }
}
