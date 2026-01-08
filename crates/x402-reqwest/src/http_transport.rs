use x402_rs::proto;
use x402_rs::proto::{v1, v2};
use x402_rs::util::Base64Bytes;

pub async fn http_payment_required_from_response(
    response: reqwest::Response,
) -> Option<proto::PaymentRequired> {
    let headers = response.headers();
    let v2_payment_required = headers
        .get("Payment-Required")
        .and_then(|h| Base64Bytes::from(h.as_bytes()).decode().ok())
        .and_then(|b| serde_json::from_slice::<v2::PaymentRequired>(&b).ok());
    if let Some(v2_payment_required) = v2_payment_required {
        return Some(proto::PaymentRequired::V2(v2_payment_required));
    }
    let v1_payment_required = response
        .bytes()
        .await
        .ok()
        .and_then(|b| serde_json::from_slice::<v1::PaymentRequired>(&b).ok());
    if let Some(v1_payment_required) = v1_payment_required {
        return Some(proto::PaymentRequired::V1(v1_payment_required));
    }
    None
}
