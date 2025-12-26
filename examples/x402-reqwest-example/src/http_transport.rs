use bytes::Bytes;
use x402_rs::util::Base64Bytes;

/// 402 Payment Required response
pub enum PaymentQuote {
    /// In body
    V1 {
        body: Bytes
    },
    /// In `Payment-Required` header
    V2 {
        payment_required_header: Bytes
    },
}

impl PaymentQuote {
    pub async fn from_response(response: reqwest::Response) -> Option<Self> {
        let headers = response.headers();
        let v2_payment_required = headers.get("Payment-Required").and_then(|h| Base64Bytes::from(h.as_bytes()).decode().ok()).map(|b| Bytes::from_owner(b));
        if let Some(v2_payment_required) = v2_payment_required {
            return Some(Self::V2 { payment_required_header: v2_payment_required });
        }
        let v1_payment_required = response.bytes().await.ok().map(|b| Bytes::from_owner(b));
        if let Some(v1_payment_required) = v1_payment_required {
            return Some(Self::V1 { body: v1_payment_required });
        }
        None
    }
}
