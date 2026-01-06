use axum::body::Body;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use serde::Serialize;
use std::fmt::Display;
use x402_rs::proto::{v1, v2};

pub trait HasPaymentRequired {
    type PaymentRequired;
}

impl HasPaymentRequired for v1::PaymentRequirements {
    type PaymentRequired = v1::PaymentRequired;
}

impl HasPaymentRequired for v2::PaymentRequirements {
    type PaymentRequired = v2::PaymentRequired;
}

/// Wrapper for producing a `402 Payment Required` response with context.
#[derive(Debug)]
pub struct PaygateError<T: HasPaymentRequired>(T::PaymentRequired);

/// Middleware application error with detailed context.
///
/// Encapsulates a `402 Payment Required` response that can be returned
/// when payment verification or settlement fails.
impl PaygateError<v1::PaymentRequirements> {
    pub fn payment_header_required(payment_requirements: Vec<v1::PaymentRequirements>) -> Self {
        let payment_required_response = v1::PaymentRequired {
            error: Some("X-PAYMENT header is required".to_string()),
            accepts: payment_requirements,
            x402_version: v1::X402Version1,
        };
        Self(payment_required_response)
    }

    pub fn invalid_payment_header(payment_requirements: Vec<v1::PaymentRequirements>) -> Self {
        let payment_required_response = v1::PaymentRequired {
            error: Some("Invalid or malformed payment header".to_string()),
            accepts: payment_requirements,
            x402_version: v1::X402Version1,
        };
        Self(payment_required_response)
    }

    pub fn no_payment_matching(payment_requirements: Vec<v1::PaymentRequirements>) -> Self {
        let payment_required_response = v1::PaymentRequired {
            error: Some("Unable to find matching payment requirements".to_string()),
            accepts: payment_requirements,
            x402_version: v1::X402Version1,
        };
        Self(payment_required_response)
    }

    pub fn verification_failed<E2: Display>(
        error: E2,
        payment_requirements: Vec<v1::PaymentRequirements>,
    ) -> Self {
        let payment_required_response = v1::PaymentRequired {
            error: Some(format!("Verification Failed: {error}")),
            accepts: payment_requirements,
            x402_version: v1::X402Version1,
        };
        Self(payment_required_response)
    }

    // FIXME When settlement is failed we should return { error: "Settlement Failed", details: "Some error details" }"
    pub fn settlement_failed<E2: Display>(error: E2) -> Self {
        let payment_required_response = v1::PaymentRequired {
            error: Some(format!("Settlement Failed: {error}")),
            accepts: vec![],
            x402_version: v1::X402Version1,
        };
        Self(payment_required_response)
    }
}

impl<T: HasPaymentRequired> IntoResponse for PaygateError<T>
where
    T::PaymentRequired: Serialize,
{
    fn into_response(self) -> Response {
        let payment_required_response_bytes =
            serde_json::to_vec(&self.0).expect("serialization failed");
        let body = Body::from(payment_required_response_bytes);
        Response::builder()
            .status(StatusCode::PAYMENT_REQUIRED)
            .header("Content-Type", "application/json")
            .body(body)
            .expect("Fail to construct response")
    }
}
