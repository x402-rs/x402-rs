//! Wire format types for the V2 EIP-155 `batch-settlement` payment scheme.
//!
//! Mirrors `typescript/packages/mechanisms/evm/src/batch-settlement/types.ts`.
//! Every `serde`-derived type round-trips against the canonical JSON wire format
//! used by the TypeScript and Go reference implementations.
//!
//! Numeric fields (`amount`, `maxClaimableAmount`, `nonce`, …) serialize as
//! **decimal** strings via the `decimal_u128` / `decimal_u256` helpers — the
//! default `alloy_primitives::Uint` `Serialize` impl emits `0x`-hex which is
//! not compatible with the spec wire format.

use alloy_primitives::{B256, Bytes, U128, U256};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use std::fmt::{self, Formatter};
use x402_types::lit_str;
use x402_types::proto::v2;

use crate::chain::ChecksummedAddress;

lit_str!(BatchSettlementScheme, "batch-settlement");

/// V2 `PaymentRequirements` for batch-settlement payments.
pub type PaymentRequirements = v2::PaymentRequirements<
    BatchSettlementScheme,
    U256String,
    ChecksummedAddress,
    BatchSettlementPaymentRequirementsExtra,
>;

/// V2 `PaymentPayload` enveloping a batch-settlement payload of any variant.
pub type PaymentPayload = v2::PaymentPayload<PaymentRequirements, BatchSettlementPayload>;

/// V2 `VerifyRequest` for batch-settlement payments.
pub type VerifyRequest = v2::VerifyRequest<PaymentPayload, PaymentRequirements>;

/// V2 `SettleRequest` for batch-settlement payments (same shape as verify).
pub type SettleRequest = VerifyRequest;

/// Newtype around `U256` that serializes as a decimal string.
///
/// Used wherever the wire format requires a decimal-string amount field
/// (per-request `PaymentRequirements.amount`, ERC-3009 `validAfter` / `validBefore`,
/// Permit2 `nonce` / `deadline`, refund nonces, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct U256String(pub U256);

impl From<U256> for U256String {
    fn from(value: U256) -> Self {
        Self(value)
    }
}

impl From<U256String> for U256 {
    fn from(value: U256String) -> Self {
        value.0
    }
}

impl Serialize for U256String {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for U256String {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        U256::from_str_radix(&s, 10)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for U256String {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Newtype around `U128` that serializes as a decimal string.
///
/// Used wherever the wire format requires a decimal-string `uint128` (channel
/// balances, claim amounts, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct U128String(pub U128);

impl From<U128> for U128String {
    fn from(value: U128) -> Self {
        Self(value)
    }
}

impl From<U128String> for U128 {
    fn from(value: U128String) -> Self {
        value.0
    }
}

impl Serialize for U128String {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for U128String {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        U128::from_str_radix(&s, 10)
            .map(Self)
            .map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for U128String {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Channel snapshot returned in `extra.channelState` on verify / settle responses
/// and in corrective 402 `extra` blocks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelStateExtra {
    pub channel_id: B256,
    pub balance: U128String,
    pub total_claimed: U128String,
    pub withdraw_requested_at: u64,
    pub refund_nonce: U256String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub charged_cumulative_amount: Option<U128String>,
}

/// Corrective voucher snapshot included alongside `channelState` in a
/// `invalid_batch_settlement_evm_cumulative_amount_mismatch` 402.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherStateExtra {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_max_claimable: Option<U128String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<Bytes>,
}

/// Asset transfer method selector hint included in `PaymentRequirements.extra`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AssetTransferMethod {
    Eip3009,
    Permit2,
}

impl fmt::Display for AssetTransferMethod {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            AssetTransferMethod::Eip3009 => f.write_str("eip3009"),
            AssetTransferMethod::Permit2 => f.write_str("permit2"),
        }
    }
}

/// `PaymentRequirements.extra` for batch-settlement payments.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSettlementPaymentRequirementsExtra {
    pub receiver_authorizer: ChecksummedAddress,
    pub withdraw_delay: u64,
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asset_transfer_method: Option<AssetTransferMethod>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_state: Option<ChannelStateExtra>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub voucher_state: Option<VoucherStateExtra>,
}

/// Immutable channel configuration; its EIP-712 hash under the
/// `x402 Batch Settlement` domain is the canonical `channelId`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelConfig {
    pub payer: ChecksummedAddress,
    pub payer_authorizer: ChecksummedAddress,
    pub receiver: ChecksummedAddress,
    pub receiver_authorizer: ChecksummedAddress,
    pub token: ChecksummedAddress,
    pub withdraw_delay: u64,
    pub salt: B256,
}

/// Per-voucher fields shared by deposit, voucher, and refund payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherFields {
    pub channel_id: B256,
    pub max_claimable_amount: U128String,
    pub signature: Bytes,
}

/// ERC-3009 `ReceiveWithAuthorization` segment used inside a deposit payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Erc3009Authorization {
    pub valid_after: U256String,
    pub valid_before: U256String,
    pub salt: B256,
    pub signature: Bytes,
}

/// Permit2 authorization segment used inside a deposit payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Authorization {
    pub from: ChecksummedAddress,
    pub permitted: Permit2Permitted,
    pub spender: ChecksummedAddress,
    pub nonce: U256String,
    pub deadline: U256String,
    pub witness: Permit2Witness,
    pub signature: Bytes,
}

/// Token permission segment of a Permit2 authorization.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Permitted {
    pub token: ChecksummedAddress,
    pub amount: U256String,
}

/// Permit2 deposit witness: binds the authorization to a single channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Permit2Witness {
    pub channel_id: B256,
}

/// Exactly-one wrapper around the two supported deposit authorization variants.
///
/// The wire shape is an object with exactly one of `erc3009Authorization` or
/// `permit2Authorization`; the other key MUST be absent (mirrors the TS
/// discriminated union — see `BatchSettlementDepositAuthorization`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DepositAuthorization {
    Erc3009(Erc3009Authorization),
    Permit2(Permit2Authorization),
}

impl Serialize for DepositAuthorization {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        #[derive(Serialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire<'a> {
            #[serde(skip_serializing_if = "Option::is_none")]
            erc3009_authorization: Option<&'a Erc3009Authorization>,
            #[serde(skip_serializing_if = "Option::is_none")]
            permit2_authorization: Option<&'a Permit2Authorization>,
        }
        let wire = match self {
            DepositAuthorization::Erc3009(a) => Wire {
                erc3009_authorization: Some(a),
                permit2_authorization: None,
            },
            DepositAuthorization::Permit2(a) => Wire {
                erc3009_authorization: None,
                permit2_authorization: Some(a),
            },
        };
        wire.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for DepositAuthorization {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            #[serde(default)]
            erc3009_authorization: Option<Erc3009Authorization>,
            #[serde(default)]
            permit2_authorization: Option<Permit2Authorization>,
        }
        let wire = Wire::deserialize(deserializer)?;
        match (wire.erc3009_authorization, wire.permit2_authorization) {
            (Some(a), None) => Ok(DepositAuthorization::Erc3009(a)),
            (None, Some(a)) => Ok(DepositAuthorization::Permit2(a)),
            (Some(_), Some(_)) => Err(D::Error::custom(
                "deposit.authorization must include exactly one of \
                 erc3009Authorization or permit2Authorization",
            )),
            (None, None) => Err(D::Error::custom(
                "deposit.authorization must include either erc3009Authorization \
                 or permit2Authorization",
            )),
        }
    }
}

/// Deposit segment: the amount transferred into the channel and how it is collected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositSegment {
    pub amount: U256String,
    pub authorization: DepositAuthorization,
}

/// `type: "deposit"` payload: first request (or top-up) creates / extends the
/// channel via the canonical deposit collector and authorizes the accompanying
/// voucher.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DepositPayload {
    pub channel_config: ChannelConfig,
    pub voucher: VoucherFields,
    pub deposit: DepositSegment,
}

/// `type: "voucher"` payload: steady-state cumulative voucher against an
/// existing channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherPayload {
    pub channel_config: ChannelConfig,
    pub voucher: VoucherFields,
}

/// `type: "refund"` payload (client form): cooperative refund request authored
/// by the client. The optional `amount` requests a partial refund; if omitted,
/// the server resolves it to a full refund before forwarding the enriched
/// payload to the facilitator.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RefundPayload {
    pub channel_config: ChannelConfig,
    pub voucher: VoucherFields,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub amount: Option<U256String>,
}

/// A claim row consumed by `claimWithSignature` / `claim` onchain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherClaim {
    pub voucher: VoucherClaimVoucher,
    pub signature: Bytes,
    pub total_claimed: U128String,
}

/// Voucher segment of a claim row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoucherClaimVoucher {
    pub channel: ChannelConfig,
    pub max_claimable_amount: U128String,
}

/// `type: "claim"` settle payload authored by the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaimPayload {
    pub claims: Vec<VoucherClaim>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_authorizer_signature: Option<Bytes>,
}

/// `type: "settle"` settle payload authored by the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettlePayload {
    pub receiver: ChecksummedAddress,
    pub token: ChecksummedAddress,
}

/// `type: "refund"` settle payload after the server enriches the client's
/// refund payload with the resolved amount, nonce, claim batch, and any
/// receiver-authorizer signatures it owns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnrichedRefundPayload {
    pub channel_config: ChannelConfig,
    pub voucher: VoucherFields,
    pub amount: U256String,
    pub refund_nonce: U256String,
    #[serde(default)]
    pub claims: Vec<VoucherClaim>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refund_authorizer_signature: Option<Bytes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim_authorizer_signature: Option<Bytes>,
}

/// Refund payload variant: either the bare client-side request or the
/// enriched server-side settlement payload. Distinguished by the presence of
/// `refundNonce`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchSettlementRefundPayload {
    Client(RefundPayload),
    Enriched(EnrichedRefundPayload),
}

impl Serialize for BatchSettlementRefundPayload {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            BatchSettlementRefundPayload::Client(p) => p.serialize(serializer),
            BatchSettlementRefundPayload::Enriched(p) => p.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for BatchSettlementRefundPayload {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        // Materialize the JSON value once so we can dispatch on the presence
        // of the enriched fields. The enriched form always carries
        // `refundNonce`; the bare client form never does.
        let value = serde_json::Value::deserialize(deserializer)?;
        let has_refund_nonce = value
            .as_object()
            .map(|obj| obj.contains_key("refundNonce"))
            .unwrap_or(false);
        if has_refund_nonce {
            serde_json::from_value::<EnrichedRefundPayload>(value)
                .map(BatchSettlementRefundPayload::Enriched)
                .map_err(D::Error::custom)
        } else {
            serde_json::from_value::<RefundPayload>(value)
                .map(BatchSettlementRefundPayload::Client)
                .map_err(D::Error::custom)
        }
    }
}

impl BatchSettlementRefundPayload {
    /// Returns the channel config shared by both refund variants.
    pub fn channel_config(&self) -> &ChannelConfig {
        match self {
            BatchSettlementRefundPayload::Client(p) => &p.channel_config,
            BatchSettlementRefundPayload::Enriched(p) => &p.channel_config,
        }
    }

    /// Returns the voucher fields shared by both refund variants.
    pub fn voucher(&self) -> &VoucherFields {
        match self {
            BatchSettlementRefundPayload::Client(p) => &p.voucher,
            BatchSettlementRefundPayload::Enriched(p) => &p.voucher,
        }
    }
}

/// Discriminated union of every payload variant the facilitator can receive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum BatchSettlementPayload {
    Deposit(DepositPayload),
    Voucher(VoucherPayload),
    Refund(BatchSettlementRefundPayload),
    Claim(ClaimPayload),
    Settle(SettlePayload),
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, b256};
    use serde_json::json;

    fn sample_channel_config() -> ChannelConfig {
        ChannelConfig {
            payer: "0x0000000000000000000000000000000000000001"
                .parse()
                .unwrap(),
            payer_authorizer: "0x0000000000000000000000000000000000000002"
                .parse()
                .unwrap(),
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

    fn sample_voucher() -> VoucherFields {
        VoucherFields {
            channel_id: b256!("0xabc123abc123abc123abc123abc123abc123abc123abc123abc123abc1230000"),
            max_claimable_amount: U128::from(1_000u128).into(),
            signature: Bytes::from_static(&[0xde, 0xad, 0xbe, 0xef]),
        }
    }

    #[test]
    fn u256_string_serializes_as_decimal() {
        let v = U256String(U256::from(100_000u64));
        let json = serde_json::to_value(v).unwrap();
        assert_eq!(json, json!("100000"));
        let back: U256String = serde_json::from_value(json).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn u128_string_serializes_as_decimal() {
        let v = U128String(U128::from(42u128));
        let json = serde_json::to_value(v).unwrap();
        assert_eq!(json, json!("42"));
        let back: U128String = serde_json::from_value(json).unwrap();
        assert_eq!(back, v);
    }

    #[test]
    fn u128_string_rejects_hex_input() {
        // The decimal parser must reject a `0x`-prefixed input — anything
        // outside [0-9] is an out-of-range digit. Compatibility with
        // hex-style strings would silently truncate the payload's amount.
        let err = serde_json::from_value::<U128String>(json!("0x64")).unwrap_err();
        let s = err.to_string();
        assert!(
            s.contains("out of range") || s.contains("invalid"),
            "expected decimal parse rejection, got: {s}"
        );
    }

    #[test]
    fn deposit_payload_round_trips_with_erc3009() {
        let payload = BatchSettlementPayload::Deposit(DepositPayload {
            channel_config: sample_channel_config(),
            voucher: sample_voucher(),
            deposit: DepositSegment {
                amount: U256::from(1_000u64).into(),
                authorization: DepositAuthorization::Erc3009(Erc3009Authorization {
                    valid_after: U256::ZERO.into(),
                    valid_before: U256::from(1_770_000_000u64).into(),
                    salt: B256::repeat_byte(0x11),
                    signature: Bytes::from_static(&[0x01, 0x02]),
                }),
            },
        });
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["type"], "deposit");
        assert_eq!(json["deposit"]["amount"], "1000");
        assert_eq!(
            json["deposit"]["authorization"]["erc3009Authorization"]["validBefore"],
            "1770000000"
        );
        assert!(json["deposit"]["authorization"]["permit2Authorization"].is_null());
        let back: BatchSettlementPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, payload);
    }

    #[test]
    fn deposit_payload_round_trips_with_permit2() {
        let payload = BatchSettlementPayload::Deposit(DepositPayload {
            channel_config: sample_channel_config(),
            voucher: sample_voucher(),
            deposit: DepositSegment {
                amount: U256::from(2_000u64).into(),
                authorization: DepositAuthorization::Permit2(Permit2Authorization {
                    from: "0x0000000000000000000000000000000000000001"
                        .parse()
                        .unwrap(),
                    permitted: Permit2Permitted {
                        token: "0x0000000000000000000000000000000000000005"
                            .parse()
                            .unwrap(),
                        amount: U256::from(2_000u64).into(),
                    },
                    spender: "0x4020425fAf3b746C082C2f942b4E5159887b0005"
                        .parse()
                        .unwrap(),
                    nonce: U256::from(42u64).into(),
                    deadline: U256::from(1_770_000_000u64).into(),
                    witness: Permit2Witness {
                        channel_id: sample_voucher().channel_id,
                    },
                    signature: Bytes::from_static(&[0x03, 0x04]),
                }),
            },
        });
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["type"], "deposit");
        assert_eq!(
            json["deposit"]["authorization"]["permit2Authorization"]["nonce"],
            "42"
        );
        assert!(json["deposit"]["authorization"]["erc3009Authorization"].is_null());
        let back: BatchSettlementPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, payload);
    }

    #[test]
    fn deposit_authorization_rejects_both_variants_present() {
        let json = json!({
            "erc3009Authorization": {
                "validAfter": "0",
                "validBefore": "1",
                "salt": "0x0000000000000000000000000000000000000000000000000000000000000000",
                "signature": "0x"
            },
            "permit2Authorization": {
                "from": "0x0000000000000000000000000000000000000001",
                "permitted": { "token": "0x0000000000000000000000000000000000000005", "amount": "1" },
                "spender": "0x4020425fAf3b746C082C2f942b4E5159887b0005",
                "nonce": "0",
                "deadline": "0",
                "witness": { "channelId": "0x0000000000000000000000000000000000000000000000000000000000000000" },
                "signature": "0x"
            }
        });
        let err = serde_json::from_value::<DepositAuthorization>(json).unwrap_err();
        assert!(err.to_string().contains("exactly one"), "{err}");
    }

    #[test]
    fn deposit_authorization_rejects_neither_variant_present() {
        let err = serde_json::from_value::<DepositAuthorization>(json!({})).unwrap_err();
        assert!(err.to_string().contains("must include"));
    }

    #[test]
    fn voucher_payload_round_trips() {
        let payload = BatchSettlementPayload::Voucher(VoucherPayload {
            channel_config: sample_channel_config(),
            voucher: sample_voucher(),
        });
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["type"], "voucher");
        assert_eq!(json["voucher"]["maxClaimableAmount"], "1000");
        let back: BatchSettlementPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, payload);
    }

    #[test]
    fn refund_payload_client_round_trips() {
        let payload =
            BatchSettlementPayload::Refund(BatchSettlementRefundPayload::Client(RefundPayload {
                channel_config: sample_channel_config(),
                voucher: sample_voucher(),
                amount: Some(U256::from(500u64).into()),
            }));
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["type"], "refund");
        assert_eq!(json["amount"], "500");
        assert!(json["refundNonce"].is_null());
        let back: BatchSettlementPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, payload);
    }

    #[test]
    fn refund_payload_enriched_round_trips() {
        let payload = BatchSettlementPayload::Refund(BatchSettlementRefundPayload::Enriched(
            EnrichedRefundPayload {
                channel_config: sample_channel_config(),
                voucher: sample_voucher(),
                amount: U256::from(500u64).into(),
                refund_nonce: U256::from(1u64).into(),
                claims: vec![VoucherClaim {
                    voucher: VoucherClaimVoucher {
                        channel: sample_channel_config(),
                        max_claimable_amount: U128::from(700u128).into(),
                    },
                    signature: Bytes::from_static(&[0xaa, 0xbb]),
                    total_claimed: U128::from(700u128).into(),
                }],
                refund_authorizer_signature: Some(Bytes::from_static(&[0xcc, 0xdd])),
                claim_authorizer_signature: None,
            },
        ));
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["type"], "refund");
        assert_eq!(json["refundNonce"], "1");
        assert_eq!(json["claims"][0]["totalClaimed"], "700");
        let back: BatchSettlementPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, payload);
    }

    #[test]
    fn claim_payload_round_trips() {
        let payload = BatchSettlementPayload::Claim(ClaimPayload {
            claims: vec![VoucherClaim {
                voucher: VoucherClaimVoucher {
                    channel: sample_channel_config(),
                    max_claimable_amount: U128::from(5_000u128).into(),
                },
                signature: Bytes::from_static(&[0x11]),
                total_claimed: U128::from(5_000u128).into(),
            }],
            claim_authorizer_signature: Some(Bytes::from_static(&[0x22])),
        });
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["type"], "claim");
        let back: BatchSettlementPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, payload);
    }

    #[test]
    fn settle_payload_round_trips() {
        let payload = BatchSettlementPayload::Settle(SettlePayload {
            receiver: "0x0000000000000000000000000000000000000003"
                .parse()
                .unwrap(),
            token: "0x0000000000000000000000000000000000000005"
                .parse()
                .unwrap(),
        });
        let json = serde_json::to_value(&payload).unwrap();
        assert_eq!(json["type"], "settle");
        let back: BatchSettlementPayload = serde_json::from_value(json).unwrap();
        assert_eq!(back, payload);
    }

    #[test]
    fn payment_requirements_extra_round_trips() {
        let extra = BatchSettlementPaymentRequirementsExtra {
            receiver_authorizer: "0x0000000000000000000000000000000000000004"
                .parse()
                .unwrap(),
            withdraw_delay: 900,
            name: "USDC".to_string(),
            version: "2".to_string(),
            asset_transfer_method: Some(AssetTransferMethod::Eip3009),
            channel_state: Some(ChannelStateExtra {
                channel_id: B256::repeat_byte(0x55),
                balance: U128::from(100_000u128).into(),
                total_claimed: U128::from(3_200u128).into(),
                withdraw_requested_at: 0,
                refund_nonce: U256::from(1u64).into(),
                charged_cumulative_amount: Some(U128::from(3_900u128).into()),
            }),
            voucher_state: Some(VoucherStateExtra {
                signed_max_claimable: Some(U128::from(3_900u128).into()),
                signature: Some(Bytes::from_static(&[0xab, 0xcd])),
            }),
        };
        let json = serde_json::to_value(&extra).unwrap();
        assert_eq!(
            json["receiverAuthorizer"],
            "0x0000000000000000000000000000000000000004"
        );
        assert_eq!(json["assetTransferMethod"], "eip3009");
        assert_eq!(json["channelState"]["chargedCumulativeAmount"], "3900");
        assert_eq!(json["channelState"]["balance"], "100000");
        let back: BatchSettlementPaymentRequirementsExtra = serde_json::from_value(json).unwrap();
        assert_eq!(back, extra);
    }

    #[test]
    fn unknown_payload_type_is_rejected() {
        let json = json!({ "type": "withdraw" });
        let err = serde_json::from_value::<BatchSettlementPayload>(json).unwrap_err();
        assert!(err.to_string().contains("withdraw"), "{err}");
    }

    #[test]
    fn batch_settlement_scheme_literal_round_trips() {
        let scheme: BatchSettlementScheme = "batch-settlement".parse().unwrap();
        assert_eq!(scheme.to_string(), "batch-settlement");
        let json = serde_json::to_value(scheme).unwrap();
        assert_eq!(json, json!("batch-settlement"));
        let back: BatchSettlementScheme = serde_json::from_value(json).unwrap();
        assert_eq!(back, scheme);
        assert!("exact".parse::<BatchSettlementScheme>().is_err());
    }

    /// Address fields on `ChannelConfig` must serialize as EIP-55 checksummed
    /// hex so the chain-bound EIP-712 hash matches the reference implementations.
    #[test]
    fn channel_config_serializes_checksummed_addresses() {
        let cfg = ChannelConfig {
            payer: Address::from_word(B256::repeat_byte(0xab)).into(),
            payer_authorizer: Address::from_word(B256::repeat_byte(0xcd)).into(),
            receiver: Address::from_word(B256::repeat_byte(0xef)).into(),
            receiver_authorizer: Address::from_word(B256::repeat_byte(0x12)).into(),
            token: Address::from_word(B256::repeat_byte(0x34)).into(),
            withdraw_delay: 900,
            salt: B256::ZERO,
        };
        let json = serde_json::to_value(&cfg).unwrap();
        let payer = json["payer"].as_str().unwrap();
        assert!(
            payer.chars().any(|c| c.is_ascii_uppercase()),
            "payer should be checksummed: {payer}"
        );
    }
}
