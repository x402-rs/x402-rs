use std::fmt::Display;
use axum::body::Body;
use axum::response::{IntoResponse, Response};
use http::StatusCode;
use x402_rs::proto::v1;

/// Wrapper for producing a `402 Payment Required` response with context.
#[derive(Debug)]
pub struct X402Error(v1::PaymentRequired);

static ERR_PAYMENT_HEADER_REQUIRED: &'static str = "X-PAYMENT header is required";
static ERR_INVALID_PAYMENT_HEADER: &'static str = "Invalid or malformed payment header";
static ERR_NO_PAYMENT_MATCHING: &'static str = "Unable to find matching payment requirements";

/// Middleware application error with detailed context.
///
/// Encapsulates a `402 Payment Required` response that can be returned
/// when payment verification or settlement fails.
impl X402Error {
    /// Direct constructor for when we already have a PaymentRequired response
    pub fn from_payment_required(payment_required: v1::PaymentRequired) -> Self {
        Self(payment_required)
    }

    pub fn payment_header_required(payment_requirements: Vec<v1::PaymentRequirements>) -> Self {
        let payment_required_response = v1::PaymentRequired {
            error: Some(ERR_PAYMENT_HEADER_REQUIRED.to_string()),
            accepts: payment_requirements,
            x402_version: v1::X402Version1,
        };
        Self(payment_required_response)
    }

    pub fn invalid_payment_header(payment_requirements: Vec<v1::PaymentRequirements>) -> Self {
        let payment_required_response = v1::PaymentRequired {
            error: Some(ERR_INVALID_PAYMENT_HEADER.to_string()),
            accepts: payment_requirements,
            x402_version: v1::X402Version1,
        };
        Self(payment_required_response)
    }

    pub fn no_payment_matching(payment_requirements: Vec<v1::PaymentRequirements>) -> Self {
        let payment_required_response = v1::PaymentRequired {
            error: Some(ERR_NO_PAYMENT_MATCHING.to_string()),
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

impl IntoResponse for X402Error {
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
