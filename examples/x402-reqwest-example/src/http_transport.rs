use x402_rs::proto;
use x402_rs::proto::{v1, v2};
use x402_rs::util::Base64Bytes;

/// 402 Payment Required response
pub enum HttpPaymentRequired {
    /// In body
    V1 { body: proto::PaymentRequired },
    /// In `Payment-Required` header
    V2 {
        payment_required_header: proto::PaymentRequired,
    },
}

impl HttpPaymentRequired {
    pub async fn from_response(response: reqwest::Response) -> Option<Self> {
        let headers = response.headers();
        let v2_payment_required = headers
            .get("Payment-Required")
            .and_then(|h| Base64Bytes::from(h.as_bytes()).decode().ok())
            .and_then(|b| serde_json::from_slice::<v2::PaymentRequired>(&b).ok());
        if let Some(v2_payment_required) = v2_payment_required {
            return Some(Self::V2 {
                payment_required_header: proto::PaymentRequired::V2(v2_payment_required),
            });
        }
        let v1_payment_required = response
            .bytes()
            .await
            .ok()
            .and_then(|b| serde_json::from_slice::<v1::PaymentRequired>(&b).ok());
        if let Some(v1_payment_required) = v1_payment_required {
            return Some(Self::V1 {
                body: proto::PaymentRequired::V1(v1_payment_required),
            });
        }
        None
    }

    pub fn as_payment_required(&self) -> &proto::PaymentRequired {
        match self {
            HttpPaymentRequired::V1 { body } => body,
            HttpPaymentRequired::V2 {
                payment_required_header,
            } => payment_required_header,
        }
    }
}
