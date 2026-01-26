//! HTTP endpoints implemented by the x402 **facilitator**.
//!
//! These are the server-side handlers for processing client-submitted x402 payments.
//! They include both protocol-critical endpoints (`/verify`, `/settle`) and discovery endpoints (`/supported`, etc).
//!
//! All payloads follow the types defined in the `x402-rs` crate, and are compatible
//! with the TypeScript and Go client SDKs.
//!
//! Each endpoint consumes or produces structured JSON payloads defined in `x402-rs`,
//! and is compatible with official x402 client SDKs.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router, response::IntoResponse};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::instrument;
use x402_types::proto;
use x402_types::proto::{AsPaymentProblem, ErrorReason};
use x402_types::scheme::X402SchemeFacilitatorError;

use crate::facilitator::Facilitator;
use crate::facilitator_local::FacilitatorLocalError;

/// `GET /verify`: Returns a machine-readable description of the `/verify` endpoint.
///
/// This is served by the facilitator to help clients understand how to construct
/// a valid [`VerifyRequest`] for payment verification.
///
/// This is optional metadata and primarily useful for discoverability and debugging tools.
#[instrument(skip_all)]
pub async fn get_verify_info() -> impl IntoResponse {
    Json(json!({
        "endpoint": "/verify",
        "description": "POST to verify x402 payments",
        "body": {
            "paymentPayload": "PaymentPayload",
            "paymentRequirements": "PaymentRequirements",
        }
    }))
}

/// `GET /settle`: Returns a machine-readable description of the `/settle` endpoint.
///
/// This is served by the facilitator to describe the structure of a valid
/// [`SettleRequest`] used to initiate on-chain payment settlement.
#[instrument(skip_all)]
pub async fn get_settle_info() -> impl IntoResponse {
    Json(json!({
        "endpoint": "/settle",
        "description": "POST to settle x402 payments",
        "body": {
            "paymentPayload": "PaymentPayload",
            "paymentRequirements": "PaymentRequirements",
        }
    }))
}

pub fn routes<A>() -> Router<A>
where
    A: Facilitator + Clone + Send + Sync + 'static,
    A::Error: IntoResponse,
{
    Router::new()
        .route("/", get(get_root))
        .route("/verify", get(get_verify_info))
        .route("/verify", post(post_verify::<A>))
        .route("/settle", get(get_settle_info))
        .route("/settle", post(post_settle::<A>))
        .route("/health", get(get_health::<A>))
        .route("/supported", get(get_supported::<A>))
}

/// `GET /`: Returns a simple greeting message from the facilitator.
#[instrument(skip_all)]
pub async fn get_root() -> impl IntoResponse {
    let pkg_name = env!("CARGO_PKG_NAME");
    (StatusCode::OK, format!("Hello from {pkg_name}!"))
}

/// `GET /supported`: Lists the x402 payment schemes and networks supported by this facilitator.
///
/// Facilitators may expose this to help clients dynamically configure their payment requests
/// based on available network and scheme support.
#[instrument(skip_all)]
pub async fn get_supported<A>(State(facilitator): State<A>) -> impl IntoResponse
where
    A: Facilitator,
    A::Error: IntoResponse,
{
    match facilitator.supported().await {
        Ok(supported) => (StatusCode::OK, Json(json!(supported))).into_response(),
        Err(error) => error.into_response(),
    }
}

#[instrument(skip_all)]
pub async fn get_health<A>(State(facilitator): State<A>) -> impl IntoResponse
where
    A: Facilitator,
    A::Error: IntoResponse,
{
    get_supported(State(facilitator)).await
}

/// `POST /verify`: Facilitator-side verification of a proposed x402 payment.
///
/// This endpoint checks whether a given payment payload satisfies the declared
/// [`PaymentRequirements`], including signature validity, scheme match, and fund sufficiency.
///
/// Responds with a [`VerifyResponse`] indicating whether the payment can be accepted.
#[instrument(skip_all)]
pub async fn post_verify<A>(
    State(facilitator): State<A>,
    Json(body): Json<proto::VerifyRequest>,
) -> impl IntoResponse
where
    A: Facilitator,
    A::Error: IntoResponse,
{
    match facilitator.verify(&body).await {
        Ok(valid_response) => (StatusCode::OK, Json(valid_response)).into_response(),
        Err(error) => {
            tracing::warn!(
                error = ?error,
                body = %serde_json::to_string(&body).unwrap_or_else(|_| "<can-not-serialize>".to_string()),
                "Verification failed"
            );
            error.into_response()
        }
    }
}

/// `POST /settle`: Facilitator-side execution of a valid x402 payment on-chain.
///
/// Given a valid [`SettleRequest`], this endpoint attempts to execute the payment
/// via ERC-3009 `transferWithAuthorization`, and returns a [`SettleResponse`] with transaction details.
///
/// This endpoint is typically called after a successful `/verify` step.
#[instrument(skip_all)]
pub async fn post_settle<A>(
    State(facilitator): State<A>,
    Json(body): Json<proto::SettleRequest>,
) -> impl IntoResponse
where
    A: Facilitator,
    A::Error: IntoResponse,
{
    match facilitator.settle(&body).await {
        Ok(valid_response) => (StatusCode::OK, Json(valid_response)).into_response(),
        Err(error) => {
            tracing::warn!(
                error = ?error,
                body = %serde_json::to_string(&body).unwrap_or_else(|_| "<can-not-serialize>".to_string()),
                "Settlement failed"
            );
            error.into_response()
        }
    }
}

impl IntoResponse for FacilitatorLocalError {
    fn into_response(self) -> Response {
        #[derive(Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct VerificationErrorResponse<'a> {
            is_valid: bool,
            invalid_reason: ErrorReason,
            invalid_reason_details: &'a str,
            payer: &'a str,
        }

        #[derive(Serialize, Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct SettlementErrorResponse<'a> {
            success: bool,
            network: &'a str,
            transaction: &'a str,
            error_reason: ErrorReason,
            error_reason_details: &'a str,
            payer: &'a str,
        }

        match self {
            FacilitatorLocalError::Verification(scheme_handler_error) => {
                let problem = scheme_handler_error.as_payment_problem();
                let verification_error_response = VerificationErrorResponse {
                    is_valid: false,
                    invalid_reason: problem.reason(),
                    invalid_reason_details: problem.details(),
                    payer: "",
                };
                let status_code = match scheme_handler_error {
                    X402SchemeFacilitatorError::PaymentVerification(_) => StatusCode::BAD_REQUEST,
                    X402SchemeFacilitatorError::OnchainFailure(_) => {
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                };
                (status_code, Json(verification_error_response)).into_response()
            }
            FacilitatorLocalError::Settlement(scheme_handler_error) => {
                let problem = scheme_handler_error.as_payment_problem();
                let settlement_error_response = SettlementErrorResponse {
                    success: false,
                    network: "",
                    transaction: "",
                    error_reason: problem.reason(),
                    error_reason_details: problem.details(),
                    payer: "",
                };
                let status_code = match scheme_handler_error {
                    X402SchemeFacilitatorError::PaymentVerification(_) => StatusCode::BAD_REQUEST,
                    X402SchemeFacilitatorError::OnchainFailure(_) => {
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                };
                (status_code, Json(settlement_error_response)).into_response()
            }
        }
    }
}
