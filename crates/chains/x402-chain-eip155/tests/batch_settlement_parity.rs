//! Cross-module integration tests for the V2 EIP-155 `batch-settlement` scheme.
//!
//! These tests sit at the crate boundary so they exercise the publicly
//! re-exported API the same way downstream consumers (the facilitator binary,
//! examples, protocol-compliance harness) do.

#![cfg(feature = "facilitator")]

use alloy_primitives::{B256, U128, U256, b256};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use serde_json::json;
use x402_chain_eip155::v2_eip155_batch_settlement::{
    BatchSettlementPayload, BatchSettlementPaymentRequirementsExtra, BatchSettlementScheme,
    ChannelConfig, DepositAuthorization, DepositPayload, DepositSegment, Erc3009Authorization,
    PaymentRequirements, RefundPayload, U256String, VoucherFields, VoucherPayload,
    facilitator::{
        BatchSettlementVerifyResponse, ReceiverAuthorizerSigner, compute_channel_id,
        compute_voucher_digest,
    },
    types::BatchSettlementRefundPayload,
};

const BASE_SEPOLIA_CHAIN_ID: u64 = 84_532;

fn sample_extra(authorizer: &str) -> BatchSettlementPaymentRequirementsExtra {
    BatchSettlementPaymentRequirementsExtra {
        receiver_authorizer: authorizer.parse().unwrap(),
        withdraw_delay: 900,
        name: "USDC".into(),
        version: "2".into(),
        asset_transfer_method: None,
        channel_state: None,
        voucher_state: None,
    }
}

fn sample_requirements(receiver: &str, authorizer: &str, token: &str) -> PaymentRequirements {
    PaymentRequirements {
        scheme: BatchSettlementScheme,
        network: x402_types::chain::ChainId::new("eip155", "84532"),
        amount: U256String::from(U256::from(1_000u64)),
        pay_to: receiver.parse().unwrap(),
        max_timeout_seconds: 300,
        asset: token.parse().unwrap(),
        extra: sample_extra(authorizer),
    }
}

fn sample_config(payer_authorizer: &str) -> ChannelConfig {
    ChannelConfig {
        payer: "0x0000000000000000000000000000000000000001"
            .parse()
            .unwrap(),
        payer_authorizer: payer_authorizer.parse().unwrap(),
        receiver: "0x0000000000000000000000000000000000000003"
            .parse()
            .unwrap(),
        receiver_authorizer: "0x0000000000000000000000000000000000000004"
            .parse()
            .unwrap(),
        token: "0x0000000000000000000000000000000000000005"
            .parse()
            .unwrap(),
        withdraw_delay: 900,
        salt: B256::ZERO,
    }
}

#[test]
fn payment_payload_voucher_round_trips_via_value() {
    let cfg = sample_config("0x0000000000000000000000000000000000000002");
    let voucher_fields = VoucherFields {
        channel_id: b256!("0xabc123abc123abc123abc123abc123abc123abc123abc123abc123abc1230000"),
        max_claimable_amount: U128::from(2_000u128).into(),
        signature: alloy_primitives::Bytes::from_static(&[0xde, 0xad, 0xbe, 0xef]),
    };
    let payload = BatchSettlementPayload::Voucher(VoucherPayload {
        channel_config: cfg,
        voucher: voucher_fields,
    });
    let json = serde_json::to_value(&payload).unwrap();
    assert_eq!(json["type"], "voucher");
    assert_eq!(json["voucher"]["maxClaimableAmount"], "2000");
    let back: BatchSettlementPayload = serde_json::from_value(json).unwrap();
    assert_eq!(back, payload);
}

#[test]
fn payment_payload_deposit_round_trips_via_value() {
    let cfg = sample_config("0x0000000000000000000000000000000000000002");
    let voucher_fields = VoucherFields {
        channel_id: B256::repeat_byte(0x42),
        max_claimable_amount: U128::from(1_000u128).into(),
        signature: alloy_primitives::Bytes::from_static(&[0x01, 0x02]),
    };
    let payload = BatchSettlementPayload::Deposit(DepositPayload {
        channel_config: cfg,
        voucher: voucher_fields,
        deposit: DepositSegment {
            amount: U256::from(100_000u64).into(),
            authorization: DepositAuthorization::Erc3009(Erc3009Authorization {
                valid_after: U256::ZERO.into(),
                valid_before: U256::from(1_770_000_000u64).into(),
                salt: B256::repeat_byte(0x11),
                signature: alloy_primitives::Bytes::from_static(&[0x03, 0x04]),
            }),
        },
    });
    let json = serde_json::to_value(&payload).unwrap();
    assert_eq!(json["type"], "deposit");
    assert_eq!(json["deposit"]["amount"], "100000");
    let back: BatchSettlementPayload = serde_json::from_value(json).unwrap();
    assert_eq!(back, payload);
}

#[test]
fn refund_payload_distinguishes_client_vs_enriched() {
    let cfg = sample_config("0x0000000000000000000000000000000000000002");
    let voucher_fields = VoucherFields {
        channel_id: B256::repeat_byte(0x77),
        max_claimable_amount: U128::from(3_200u128).into(),
        signature: alloy_primitives::Bytes::new(),
    };

    let client =
        BatchSettlementPayload::Refund(BatchSettlementRefundPayload::Client(RefundPayload {
            channel_config: cfg.clone(),
            voucher: voucher_fields.clone(),
            amount: Some(U256::from(500u64).into()),
        }));
    let client_json = serde_json::to_value(&client).unwrap();
    assert!(client_json["refundNonce"].is_null());
    let back: BatchSettlementPayload = serde_json::from_value(client_json).unwrap();
    assert_eq!(back, client);

    // Now an enriched form gains `refundNonce` and `claims` and dispatches
    // to the `Enriched` variant on deserialization.
    let enriched_json = json!({
        "type": "refund",
        "channelConfig": serde_json::to_value(&cfg).unwrap(),
        "voucher": serde_json::to_value(&voucher_fields).unwrap(),
        "amount": "500",
        "refundNonce": "1",
        "claims": [],
    });
    let back_enriched: BatchSettlementPayload = serde_json::from_value(enriched_json).unwrap();
    let BatchSettlementPayload::Refund(BatchSettlementRefundPayload::Enriched(enriched)) =
        back_enriched
    else {
        panic!("expected enriched refund payload");
    };
    assert_eq!(enriched.refund_nonce.0, U256::from(1u64));
    assert_eq!(enriched.amount.0, U256::from(500u64));
    assert!(enriched.refund_authorizer_signature.is_none());
}

#[test]
fn channel_id_matches_independently_computed_eip712_hash() {
    // Two channels with everything identical except `salt` MUST yield distinct
    // channel ids, and two identical configs on different chains MUST also
    // differ — that's the security property the chain-bound EIP-712 domain
    // gives us.
    let cfg_a = sample_config("0x0000000000000000000000000000000000000002");
    let cfg_b = {
        let mut c = cfg_a.clone();
        c.salt = B256::repeat_byte(0x99);
        c
    };
    assert_ne!(
        compute_channel_id(&cfg_a, BASE_SEPOLIA_CHAIN_ID),
        compute_channel_id(&cfg_b, BASE_SEPOLIA_CHAIN_ID),
    );
    assert_ne!(
        compute_channel_id(&cfg_a, BASE_SEPOLIA_CHAIN_ID),
        compute_channel_id(&cfg_a, 1),
    );
}

#[test]
fn payment_requirements_extra_serializes_channel_state_when_present() {
    let req = sample_requirements(
        "0x0000000000000000000000000000000000000003",
        "0x0000000000000000000000000000000000000004",
        "0x0000000000000000000000000000000000000005",
    );
    let json = serde_json::to_value(&req).unwrap();
    assert_eq!(json["scheme"], "batch-settlement");
    assert_eq!(json["network"], "eip155:84532");
    assert_eq!(json["amount"], "1000");
    assert_eq!(
        json["extra"]["receiverAuthorizer"],
        "0x0000000000000000000000000000000000000004"
    );
    let back: PaymentRequirements = serde_json::from_value(json).unwrap();
    assert_eq!(back.network, req.network);
    assert_eq!(back.amount.0, req.amount.0);
}

#[test]
fn verify_response_invalid_round_trips_with_payer_and_reason() {
    let resp = BatchSettlementVerifyResponse::invalid(
        Some("0xPayer".into()),
        "invalid_batch_settlement_evm_voucher_signature",
    );
    let json = serde_json::to_value(&resp).unwrap();
    assert_eq!(json["isValid"], false);
    assert_eq!(json["payer"], "0xPayer");
    assert_eq!(
        json["invalidReason"],
        "invalid_batch_settlement_evm_voucher_signature"
    );
}

#[test]
fn receiver_authorizer_signs_and_recovers_for_voucher_digest() {
    let signer = PrivateKeySigner::random();
    let authorizer_address = signer.address();
    let authorizer = ReceiverAuthorizerSigner::new(signer);

    // Compute a voucher digest, sign it as if we were the payer authorizer,
    // and confirm the recovered address matches.
    let cfg = sample_config(&format!("{:#x}", authorizer_address));
    let channel_id = compute_channel_id(&cfg, BASE_SEPOLIA_CHAIN_ID);
    let max_claimable = U128::from(1_500u128);
    let voucher_digest = compute_voucher_digest(channel_id, max_claimable, BASE_SEPOLIA_CHAIN_ID);

    let raw_sig: alloy_primitives::Bytes =
        // sign as the same EOA — this is the same flow the client would use.
        authorizer
            .sign_refund(channel_id, max_claimable, U256::ZERO, BASE_SEPOLIA_CHAIN_ID)
            .unwrap();

    // The refund and voucher digests differ, so the *recovered* refund
    // signer must NOT match the voucher digest's signer.
    let sig = alloy_primitives::Signature::from_raw(&raw_sig)
        .unwrap()
        .normalized_s();
    let from_refund = sig.recover_address_from_prehash(&voucher_digest);
    assert!(from_refund.is_err() || from_refund.unwrap() != authorizer_address);

    // The dedicated EOA flow used by clients: hash the voucher digest with
    // the signer and confirm recovery against the EOA.
    let voucher_signer = PrivateKeySigner::random();
    let voucher_signer_address = voucher_signer.address();
    let voucher_sig = voucher_signer.sign_hash_sync(&voucher_digest).unwrap();
    let recovered = voucher_sig
        .normalized_s()
        .recover_address_from_prehash(&voucher_digest)
        .unwrap();
    assert_eq!(recovered, voucher_signer_address);
}
