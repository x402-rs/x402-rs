use x402_rs::proto;
use x402_rs::proto::{v1, v2};
use x402_rs::util::Base64Bytes;

pub enum HttpTransport<A> {
    V1(A),
    V2(A),
}

pub struct HttpPaymentRequired(HttpTransport<proto::PaymentRequired>);

impl HttpPaymentRequired {
    pub async fn from_response(response: reqwest::Response) -> Option<Self> {
        let headers = response.headers();
        let v2_payment_required = headers
            .get("Payment-Required")
            .and_then(|h| Base64Bytes::from(h.as_bytes()).decode().ok())
            .and_then(|b| serde_json::from_slice::<v2::PaymentRequired>(&b).ok());
        if let Some(v2_payment_required) = v2_payment_required {
            return Some(Self(HttpTransport::V2(proto::PaymentRequired::V2(
                v2_payment_required,
            ))));
        }
        let v1_payment_required = response
            .bytes()
            .await
            .ok()
            .and_then(|b| serde_json::from_slice::<v1::PaymentRequired>(&b).ok());
        if let Some(v1_payment_required) = v1_payment_required {
            return Some(Self(HttpTransport::V1(proto::PaymentRequired::V1(
                v1_payment_required,
            ))));
        }
        None
    }

    pub fn inner(&self) -> &HttpTransport<proto::PaymentRequired> {
        &self.0
    }

    pub fn as_payment_required(&self) -> &proto::PaymentRequired {
        match &self.0 {
            HttpTransport::V1(body) => body,
            HttpTransport::V2(payment_required_header) => payment_required_header,
        }
    }
}
