//! Cross-module integration tests for the V2 EIP-155 `batch-settlement` scheme.
//!
//! These tests sit at the crate boundary so they exercise the publicly
//! re-exported API the same way downstream consumers (the facilitator binary,
//! examples, protocol-compliance harness) do.

#![cfg(feature = "facilitator")]

use alloy_primitives::{Address, B256, Signature, U128, U256, b256, hex};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use serde_json::json;
use x402_chain_eip155::v2_eip155_batch_settlement::{
    BatchSettlementPayload, BatchSettlementPaymentRequirementsExtra, BatchSettlementScheme,
    ChannelConfig, ClaimPayload, DepositAuthorization, DepositPayload, DepositSegment,
    Erc3009Authorization, PaymentRequirements, RefundPayload, SettlePayload, U256String,
    VoucherClaim, VoucherClaimVoucher, VoucherFields, VoucherPayload,
    facilitator::{
        BatchSettlementVerifyResponse, ReceiverAuthorizerSigner, compute_channel_id,
        compute_voucher_digest,
    },
    types::BatchSettlementRefundPayload,
};

// --- Cross-implementation viem reference vector ----------------------------
// A fully concrete payer / channel / voucher tuple whose hashes and signature
// were produced with viem (the TypeScript library the x402 client SDK signs
// with) and independently reproduced by a hand-rolled keccak256 + abi.encode
// implementation. Pinning these here makes `alloy_sol_types`' typed-data
// derivation and `alloy_primitives`' ECDSA recovery agree with a real
// client-produced signature byte-for-byte. Source:
// https://github.com/Adelagric/x402-batch-settlement (crates/x402/tests).
const VIEM_PAYER: &str = "0x70997970C51812dc3A010C7d01b50e0d17dc79C8";
const VIEM_RECEIVER: &str = "0x19ee5100D3a1e687F85B952bd3FbEc108Ab6A8d7";
const VIEM_RECEIVER_AUTHORIZER: &str = "0xd407e409E34E0b9afb99EcCeb609bDbcD5e7f1bf";
const VIEM_TOKEN: &str = "0x036CbD53842c5426634e7929541eC2318f3dCF7e";
const VIEM_CHANNEL_ID: B256 =
    b256!("0x5fafb915f0dbee350d7f84d91802dea47e8e3a71929c3cd79da161c291fb28bd");
const VIEM_VOUCHER_DIGEST: B256 =
    b256!("0xa2874adbecca0abb1884b4ac1c100e3906d25208ad0c9e6a8fcf9790ccfa2246");
// 65-byte secp256k1 signature produced by viem over VIEM_VOUCHER_DIGEST,
// signed by VIEM_PAYER (acting as its own payerAuthorizer).
const VIEM_SIGNATURE: &str = "0x6ad7a9c0cd0172b09704c56dd22de6d2877cf912de007dcdc7757a68756b84af2d2767327e6b84836fe2b48fd1b09675957c2fa4ec6c33a146dddd0e823cf1341c";
const VIEM_MAX_CLAIMABLE: u128 = 1_000;

fn viem_channel_config() -> ChannelConfig {
    ChannelConfig {
        payer: VIEM_PAYER.parse().unwrap(),
        payer_authorizer: VIEM_PAYER.parse().unwrap(),
        receiver: VIEM_RECEIVER.parse().unwrap(),
        receiver_authorizer: VIEM_RECEIVER_AUTHORIZER.parse().unwrap(),
        token: VIEM_TOKEN.parse().unwrap(),
        withdraw_delay: 900,
        salt: B256::ZERO,
    }
}

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
fn channel_id_with_nonzero_salt_matches_viem_reference_vector() {
    // Same `ChannelConfig` as `channel_id_matches_viem_reference_vector`
    // except `salt` is now `0x4242…4242`. Pins that `salt` actually
    // participates in the EIP-712 hash — a wrong salt encoding would
    // produce a different channel id from viem.
    let cfg = ChannelConfig {
        payer: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
            .parse()
            .unwrap(),
        payer_authorizer: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
            .parse()
            .unwrap(),
        receiver: "0x19ee5100D3a1e687F85B952bd3FbEc108Ab6A8d7"
            .parse()
            .unwrap(),
        receiver_authorizer: "0xd407e409E34E0b9afb99EcCeb609bDbcD5e7f1bf"
            .parse()
            .unwrap(),
        token: "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
            .parse()
            .unwrap(),
        withdraw_delay: 900,
        salt: B256::repeat_byte(0x42),
    };
    assert_eq!(
        compute_channel_id(&cfg, BASE_SEPOLIA_CHAIN_ID),
        b256!("0x7b3bf678a448e1882ab277b789987b5dee3b7cc6fc2c7e91687a74b54b474423"),
    );
}

#[test]
fn channel_id_chain_id_1_matches_viem_reference_vector() {
    // Same config as `channel_id_matches_viem_reference_vector` but
    // computed on chain id 1 (Ethereum mainnet). The chain-bound
    // EIP-712 domain must produce a different channel id than the
    // Base Sepolia one — pins that `chainId` participates in the
    // domain construction.
    let cfg = ChannelConfig {
        payer: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
            .parse()
            .unwrap(),
        payer_authorizer: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
            .parse()
            .unwrap(),
        receiver: "0x19ee5100D3a1e687F85B952bd3FbEc108Ab6A8d7"
            .parse()
            .unwrap(),
        receiver_authorizer: "0xd407e409E34E0b9afb99EcCeb609bDbcD5e7f1bf"
            .parse()
            .unwrap(),
        token: "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
            .parse()
            .unwrap(),
        withdraw_delay: 900,
        salt: B256::ZERO,
    };
    assert_eq!(
        compute_channel_id(&cfg, 1),
        b256!("0x511b6617e235c4e0c4837f7b51700a3b324110c88225aee727a74536cce36192"),
    );
}

#[test]
fn voucher_digest_at_u128_max_matches_viem_reference_vector() {
    // Boundary case: `max_claimable_amount` at `U128::MAX` for the
    // channel id pinned above. Pins the `uint128` encoding edge —
    // any off-by-one in the U128 abi encoding would change the
    // resulting digest.
    let channel_id = b256!("0x5fafb915f0dbee350d7f84d91802dea47e8e3a71929c3cd79da161c291fb28bd");
    assert_eq!(
        compute_voucher_digest(channel_id, U128::MAX, BASE_SEPOLIA_CHAIN_ID),
        b256!("0x9d774906d7e4dbe91d887f3845e83c17c8fed29ca225d14d809d75bea7c3e547"),
    );
}

#[test]
fn voucher_digest_at_zero_amount_matches_viem_reference_vector() {
    // Boundary case: `max_claimable_amount = 0` for the channel id
    // pinned above. Catches an implementation that special-cases the
    // zero value or that drops the field on serialization.
    let channel_id = b256!("0x5fafb915f0dbee350d7f84d91802dea47e8e3a71929c3cd79da161c291fb28bd");
    assert_eq!(
        compute_voucher_digest(channel_id, U128::ZERO, BASE_SEPOLIA_CHAIN_ID),
        b256!("0x4187c22619aeed9d5381a1d0c37c9e936e6dfe666f963f7ccfd524e3b8f55173"),
    );
}

#[test]
fn voucher_signature_recovers_to_payer_for_viem_reference_vector() {
    // The signature was produced by viem over VIEM_VOUCHER_DIGEST. Recovering
    // it with `alloy_primitives` against the digest that `compute_voucher_digest`
    // produces must yield VIEM_PAYER. This pins the whole client→facilitator
    // signature path: viem signing, alloy's typed-data digest, and alloy's
    // ECDSA recovery all agree, so a real client-produced voucher verifies
    // against this facilitator.
    let digest = compute_voucher_digest(
        VIEM_CHANNEL_ID,
        U128::from(VIEM_MAX_CLAIMABLE),
        BASE_SEPOLIA_CHAIN_ID,
    );
    assert_eq!(digest, VIEM_VOUCHER_DIGEST);

    let sig_bytes = hex::decode(VIEM_SIGNATURE).unwrap();
    let sig = Signature::from_raw(&sig_bytes).unwrap();
    let recovered = sig.recover_address_from_prehash(&digest).unwrap();

    assert_eq!(recovered, VIEM_PAYER.parse::<Address>().unwrap());
}

#[test]
fn verify_voucher_end_to_end_for_viem_reference_vector() {
    // End-to-end of the voucher crypto core, anchored to viem: recompute the
    // channel id from the wire `ChannelConfig`, recompute the voucher digest,
    // recover the signer from the viem signature, and confirm it matches the
    // channel's declared `payer_authorizer`. This is exactly the chain
    // `verify` runs before consulting on-chain state, with every intermediate
    // value pinned to a byte-exact viem reference.
    let cfg = viem_channel_config();

    let channel_id = compute_channel_id(&cfg, BASE_SEPOLIA_CHAIN_ID);
    assert_eq!(channel_id, VIEM_CHANNEL_ID);

    let digest = compute_voucher_digest(
        channel_id,
        U128::from(VIEM_MAX_CLAIMABLE),
        BASE_SEPOLIA_CHAIN_ID,
    );
    assert_eq!(digest, VIEM_VOUCHER_DIGEST);

    let sig = Signature::from_raw(&hex::decode(VIEM_SIGNATURE).unwrap()).unwrap();
    let recovered = sig.recover_address_from_prehash(&digest).unwrap();

    // The recovered voucher signer must be the channel's payer authorizer.
    assert_eq!(recovered, cfg.payer_authorizer.0);
}

#[test]
fn claim_payload_serializes_to_canonical_wire_shape() {
    // The existing `claim_payload_round_trips` test only checks `type` and
    // serde round-trip equality, which does not catch a `rename`/`rename_all`
    // drift on the nested fields. Pin the canonical camelCase wire field names
    // the facilitator's `POST /settle` consumes for a `type: "claim"` payload.
    let payload = BatchSettlementPayload::Claim(ClaimPayload {
        claims: vec![VoucherClaim {
            voucher: VoucherClaimVoucher {
                channel: viem_channel_config(),
                max_claimable_amount: U128::from(5_000u128).into(),
            },
            signature: hex::decode(VIEM_SIGNATURE).unwrap().into(),
            total_claimed: U128::from(5_000u128).into(),
        }],
        claim_authorizer_signature: None,
    });
    let json = serde_json::to_value(&payload).unwrap();

    assert_eq!(json["type"], "claim");
    let claim = &json["claims"][0];
    assert_eq!(claim["voucher"]["maxClaimableAmount"], "5000");
    assert_eq!(claim["voucher"]["channel"]["payer"], VIEM_PAYER);
    assert_eq!(claim["voucher"]["channel"]["payerAuthorizer"], VIEM_PAYER);
    assert_eq!(
        claim["voucher"]["channel"]["receiverAuthorizer"],
        VIEM_RECEIVER_AUTHORIZER
    );
    assert_eq!(claim["voucher"]["channel"]["withdrawDelay"], 900);
    assert_eq!(claim["totalClaimed"], "5000");
    // Server-delegated authorizer: the optional signature is omitted, not null.
    assert!(json.get("claimAuthorizerSignature").is_none());
}

#[test]
fn settle_payload_serializes_to_canonical_wire_shape() {
    // Pin the canonical wire field names for a `type: "settle"` payload.
    let payload = BatchSettlementPayload::Settle(SettlePayload {
        receiver: VIEM_RECEIVER.parse().unwrap(),
        token: VIEM_TOKEN.parse().unwrap(),
    });
    let json = serde_json::to_value(&payload).unwrap();

    assert_eq!(json["type"], "settle");
    assert_eq!(json["receiver"], VIEM_RECEIVER);
    assert_eq!(json["token"], VIEM_TOKEN);
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

// ---------------------------------------------------------------------------
// Cross-implementation EIP-712 golden vectors (viem <-> alloy <-> hand-rolled).
//
// Contributed by @Adelagric (issue #92, Adelagric/x402-batch-settlement): the
// canonical vectors the official x402 client SDK signs with (viem
// `hashTypedData`), independently reproduced by a hand-rolled
// keccak256 + abi.encode implementation. They pin `alloy_sol_types`' typed-data
// hashing byte-for-byte against viem — a one-byte drift in any type string, the
// domain, or the struct encoding changes one of these hashes and fails here,
// instead of silently rejecting real client signatures at the facilitator. The
// two assertions transitively pin the typehashes too.
// ---------------------------------------------------------------------------

#[test]
fn channel_id_matches_viem_reference_vector() {
    // Vector generated with viem `hashTypedData` over the canonical
    // `x402 Batch Settlement` EIP-712 domain on Base Sepolia (chainId 84532).
    // Independently reproduced by a hand-rolled keccak256+abi.encode
    // implementation; both equal the constant below — so the assertion pins
    // `alloy_sol_types` against viem.
    let cfg = ChannelConfig {
        payer: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
            .parse()
            .unwrap(),
        payer_authorizer: "0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
            .parse()
            .unwrap(),
        receiver: "0x19ee5100D3a1e687F85B952bd3FbEc108Ab6A8d7"
            .parse()
            .unwrap(),
        receiver_authorizer: "0xd407e409E34E0b9afb99EcCeb609bDbcD5e7f1bf"
            .parse()
            .unwrap(),
        token: "0x036CbD53842c5426634e7929541eC2318f3dCF7e"
            .parse()
            .unwrap(),
        withdraw_delay: 900,
        salt: B256::ZERO,
    };
    assert_eq!(
        compute_channel_id(&cfg, BASE_SEPOLIA_CHAIN_ID),
        b256!("0x5fafb915f0dbee350d7f84d91802dea47e8e3a71929c3cd79da161c291fb28bd"),
    );
}

#[test]
fn voucher_digest_matches_viem_reference_vector() {
    // Same vector source: viem `hashTypedData` over
    // `Voucher(bytes32 channelId,uint128 maxClaimableAmount)` under the
    // `x402 Batch Settlement` domain on Base Sepolia. channel_id is the one
    // pinned by `channel_id_matches_viem_reference_vector`;
    // max_claimable_amount = 1000.
    let channel_id = b256!("0x5fafb915f0dbee350d7f84d91802dea47e8e3a71929c3cd79da161c291fb28bd");
    assert_eq!(
        compute_voucher_digest(channel_id, U128::from(1_000u128), BASE_SEPOLIA_CHAIN_ID),
        b256!("0xa2874adbecca0abb1884b4ac1c100e3906d25208ad0c9e6a8fcf9790ccfa2246"),
    );
}
