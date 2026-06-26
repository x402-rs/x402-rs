//! Verify / settle response wire types for batch-settlement.
//!
//! The scheme extends the standard V2 `VerifyResponse` / `SettleResponse`
//! with an `extra` block that carries the onchain channel snapshot
//! (`channelState`). Servers consume that snapshot to keep their mirrored
//! state fresh and emit the corrective 402 when the client falls behind.

use serde::{Deserialize, Serialize};
use x402_types::proto;

use super::utils::OnchainChannelState;
use crate::v2_eip155_batch_settlement::types::{ChannelStateExtra, U128String, U256String};

/// Verify-response extra block: the onchain channel snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSettlementVerifyExtra {
    pub channel_id: alloy_primitives::B256,
    pub balance: U128String,
    pub total_claimed: U128String,
    pub withdraw_requested_at: u64,
    pub refund_nonce: U256String,
}

impl BatchSettlementVerifyExtra {
    /// Builds the verify extra from an onchain snapshot + channel id.
    pub fn from_state(channel_id: alloy_primitives::B256, state: &OnchainChannelState) -> Self {
        Self {
            channel_id,
            balance: state.balance.into(),
            total_claimed: state.total_claimed.into(),
            withdraw_requested_at: state.withdraw_requested_at,
            refund_nonce: state.refund_nonce.into(),
        }
    }
}

/// Settle-response extra block: nested `channelState` snapshot.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSettlementSettleExtra {
    pub channel_state: ChannelStateExtra,
}

/// Wire format for the batch-settlement-flavoured `VerifyResponse`.
///
/// Distinct from `proto::v1::VerifyResponse` (which has no `extra` slot).
/// Converts into [`proto::VerifyResponse`] via the standard JSON pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSettlementVerifyResponse {
    pub is_valid: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalid_message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<BatchSettlementVerifyExtra>,
}

impl BatchSettlementVerifyResponse {
    /// Constructs a success response with the channel snapshot.
    pub fn valid(payer: String, extra: BatchSettlementVerifyExtra) -> Self {
        Self {
            is_valid: true,
            payer: Some(payer),
            invalid_reason: None,
            invalid_message: None,
            extra: Some(extra),
        }
    }

    /// Constructs a verification-failure response.
    pub fn invalid(payer: Option<String>, reason: &'static str) -> Self {
        Self {
            is_valid: false,
            payer,
            invalid_reason: Some(reason.to_string()),
            invalid_message: None,
            extra: None,
        }
    }

    /// Convenience constructor that also surfaces a free-form `invalidMessage`
    /// alongside the canonical reason code.
    pub fn invalid_with_message(
        payer: Option<String>,
        reason: &'static str,
        message: String,
    ) -> Self {
        Self {
            is_valid: false,
            payer,
            invalid_reason: Some(reason.to_string()),
            invalid_message: Some(message),
            extra: None,
        }
    }
}

impl From<BatchSettlementVerifyResponse> for proto::VerifyResponse {
    fn from(value: BatchSettlementVerifyResponse) -> Self {
        proto::VerifyResponse(
            serde_json::to_value(value)
                .expect("BatchSettlementVerifyResponse serialization failed"),
        )
    }
}

/// Wire format for the batch-settlement-flavoured `SettleResponse`.
///
/// Mirrors the upstream `SettleResponse` shape used by the TypeScript
/// reference, which carries `transaction`, `payer`, `amount`, and a
/// scheme-specific `extra` block.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BatchSettlementSettleResponse {
    pub success: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,
    pub transaction: String,
    pub network: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payer: Option<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub amount: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<BatchSettlementSettleExtra>,
}

impl BatchSettlementSettleResponse {
    /// Constructs an error settle response with a canonical reason code.
    pub fn failure(network: String, reason: &'static str) -> Self {
        Self {
            success: false,
            error_reason: Some(reason.to_string()),
            error_message: None,
            transaction: String::new(),
            network,
            payer: None,
            amount: String::new(),
            extra: None,
        }
    }

    /// Same as [`failure`] but with a free-form diagnostic message.
    pub fn failure_with_message(network: String, reason: &'static str, message: String) -> Self {
        Self {
            success: false,
            error_reason: Some(reason.to_string()),
            error_message: Some(message),
            transaction: String::new(),
            network,
            payer: None,
            amount: String::new(),
            extra: None,
        }
    }
}

impl From<BatchSettlementSettleResponse> for proto::SettleResponse {
    fn from(value: BatchSettlementSettleResponse) -> Self {
        proto::SettleResponse(
            serde_json::to_value(value)
                .expect("BatchSettlementSettleResponse serialization failed"),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{B256, U128, U256};
    use serde_json::json;

    #[test]
    fn verify_valid_serializes_camel_case_with_channel_state() {
        let extra = BatchSettlementVerifyExtra {
            channel_id: B256::repeat_byte(0xaa),
            balance: U128::from(1_000u128).into(),
            total_claimed: U128::from(500u128).into(),
            withdraw_requested_at: 0,
            refund_nonce: U256::from(1u64).into(),
        };
        let resp = BatchSettlementVerifyResponse::valid("0xPayer".into(), extra);
        let json = serde_json::to_value(resp).unwrap();
        assert_eq!(json["isValid"], true);
        assert_eq!(json["payer"], "0xPayer");
        assert_eq!(json["extra"]["balance"], "1000");
        assert_eq!(json["extra"]["totalClaimed"], "500");
        assert_eq!(json["extra"]["refundNonce"], "1");
    }

    #[test]
    fn verify_invalid_omits_extra() {
        let resp = BatchSettlementVerifyResponse::invalid(
            Some("0xPayer".into()),
            "invalid_batch_settlement_evm_voucher_signature",
        );
        let json = serde_json::to_value(resp).unwrap();
        assert_eq!(json["isValid"], false);
        assert_eq!(
            json["invalidReason"],
            "invalid_batch_settlement_evm_voucher_signature"
        );
        assert!(json.get("extra").is_none() || json["extra"].is_null());
    }

    #[test]
    fn settle_response_serializes_extra() {
        let resp = BatchSettlementSettleResponse {
            success: true,
            error_reason: None,
            error_message: None,
            transaction: String::new(),
            network: "eip155:84532".into(),
            payer: Some("0xPayer".into()),
            amount: String::new(),
            extra: Some(BatchSettlementSettleExtra {
                channel_state: ChannelStateExtra {
                    channel_id: B256::repeat_byte(0xab),
                    balance: U128::from(100_000u128).into(),
                    total_claimed: U128::from(3_900u128).into(),
                    withdraw_requested_at: 0,
                    refund_nonce: U256::from(1u64).into(),
                    charged_cumulative_amount: None,
                },
            }),
        };
        let json = serde_json::to_value(resp).unwrap();
        assert_eq!(json["success"], true);
        assert_eq!(json["extra"]["channelState"]["balance"], "100000");
    }

    #[test]
    fn settle_failure_has_empty_transaction_and_amount() {
        let resp = BatchSettlementSettleResponse::failure(
            "eip155:84532".into(),
            "invalid_batch_settlement_evm_settle_simulation_failed",
        );
        let json = serde_json::to_value(resp).unwrap();
        assert_eq!(json["success"], false);
        assert_eq!(json["transaction"], "");
        assert_eq!(json["network"], "eip155:84532");
        assert_eq!(
            json["errorReason"],
            "invalid_batch_settlement_evm_settle_simulation_failed"
        );
        assert!(json.get("amount").is_none() || json["amount"] == json!(""));
    }

    #[test]
    fn verify_response_round_trips_via_value() {
        let extra = BatchSettlementVerifyExtra {
            channel_id: B256::repeat_byte(0xaa),
            balance: U128::from(1_000u128).into(),
            total_claimed: U128::from(500u128).into(),
            withdraw_requested_at: 42,
            refund_nonce: U256::from(0u64).into(),
        };
        let resp = BatchSettlementVerifyResponse::valid("0xPayer".into(), extra);
        let json = serde_json::to_value(&resp).unwrap();
        let back: BatchSettlementVerifyResponse = serde_json::from_value(json).unwrap();
        assert!(back.is_valid);
        assert_eq!(back.payer.as_deref(), Some("0xPayer"));
        assert_eq!(back.extra.unwrap().withdraw_requested_at, 42);
    }
}
