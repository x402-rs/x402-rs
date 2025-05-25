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

use axum::http::StatusCode;
use axum::{Extension, Json, response::IntoResponse};
use serde_json::json;
use tracing::instrument;

use crate::facilitator::Facilitator;
use crate::facilitator_local::{FacilitatorLocal, PaymentError};
use crate::network::Network;
use crate::types::{
    ErrorResponse, FacilitatorErrorReason, Scheme, SettleRequest, SettleResponse,
    SupportedPaymentKind, VerifyRequest, VerifyResponse, X402Version,
};

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

/// `GET /supported`: Lists the x402 payment schemes and networks supported by this facilitator.
///
/// Facilitators may expose this to help clients dynamically configure their payment requests
/// based on available network and scheme support.
#[instrument(skip_all)]
pub async fn get_supported() -> impl IntoResponse {
    let mut kinds = Vec::with_capacity(Network::variants().len());
    for network in Network::variants() {
        kinds.push(SupportedPaymentKind {
            x402_version: X402Version::V1,
            scheme: Scheme::Exact,
            network: *network,
        })
    }
    (StatusCode::OK, Json(kinds))
}

/// `POST /verify`: Facilitator-side verification of a proposed x402 payment.
///
/// This endpoint checks whether a given payment payload satisfies the declared
/// [`PaymentRequirements`], including signature validity, scheme match, and fund sufficiency.
///
/// Responds with a [`VerifyResponse`] indicating whether the payment can be accepted.
#[instrument(skip_all)]
pub async fn post_verify(
    Extension(facilitator): Extension<FacilitatorLocal>,
    Json(body): Json<VerifyRequest>,
) -> impl IntoResponse {
    let payload = &body.payment_payload;
    let payer = &payload.payload.authorization.from;

    match facilitator.verify(&body).await {
        Ok(valid_response) => (StatusCode::OK, Json(valid_response)).into_response(),
        Err(error) => {
            tracing::warn!(
                error = ?error,
                body = %serde_json::to_string(&body).unwrap_or_else(|_| "<can-not-serialize>".to_string()),
                "Verification failed"
            );
            let bad_request = (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid request".to_string(),
                }),
            )
                .into_response();

            let invalid_schema = (
                StatusCode::OK,
                Json(VerifyResponse::invalid(
                    *payer,
                    FacilitatorErrorReason::InvalidScheme,
                )),
            )
                .into_response();

            match error {
                PaymentError::IncompatibleScheme { .. }
                | PaymentError::IncompatibleNetwork { .. }
                | PaymentError::IncompatibleReceivers { .. }
                | PaymentError::InvalidSignature(_)
                | PaymentError::InvalidTiming(_)
                | PaymentError::InsufficientValue => invalid_schema,
                PaymentError::UnsupportedNetwork(_) => (
                    StatusCode::OK,
                    Json(VerifyResponse::invalid(
                        *payer,
                        FacilitatorErrorReason::InvalidNetwork,
                    )),
                )
                    .into_response(),
                PaymentError::InvalidContractCall(_)
                | PaymentError::InvalidAddress(_)
                | PaymentError::ClockError => bad_request,
                PaymentError::InsufficientFunds => (
                    StatusCode::OK,
                    Json(VerifyResponse::invalid(
                        *payer,
                        FacilitatorErrorReason::InsufficientFunds,
                    )),
                )
                    .into_response(),
            }
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
pub async fn post_settle(
    Extension(facilitator): Extension<FacilitatorLocal>,
    Json(body): Json<SettleRequest>,
) -> impl IntoResponse {
    let payer = &body.payment_payload.payload.authorization.from;
    let network = &body.payment_payload.network;
    match facilitator.settle(&body).await {
        Ok(valid_response) => (StatusCode::OK, Json(valid_response)).into_response(),
        Err(error) => {
            tracing::warn!(
                error = ?error,
                body = %serde_json::to_string(&body).unwrap_or_else(|_| "<can-not-serialize>".to_string()),
                "Settlement failed"
            );
            let bad_request = (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: "Invalid request".to_string(),
                }),
            )
                .into_response();

            let invalid_schema = (
                StatusCode::OK,
                Json(SettleResponse {
                    success: false,
                    error_reason: Some(FacilitatorErrorReason::InvalidScheme),
                    payer: (*payer).into(),
                    transaction: None,
                    network: *network,
                }),
            )
                .into_response();

            match error {
                PaymentError::IncompatibleScheme { .. }
                | PaymentError::IncompatibleNetwork { .. }
                | PaymentError::IncompatibleReceivers { .. }
                | PaymentError::InvalidSignature(_)
                | PaymentError::InvalidTiming(_)
                | PaymentError::InsufficientValue => invalid_schema,
                PaymentError::InvalidContractCall(_)
                | PaymentError::InvalidAddress(_)
                | PaymentError::UnsupportedNetwork(_)
                | PaymentError::ClockError => bad_request,
                PaymentError::InsufficientFunds => (
                    StatusCode::BAD_REQUEST,
                    Json(SettleResponse {
                        success: false,
                        error_reason: Some(FacilitatorErrorReason::InsufficientFunds),
                        payer: (*payer).into(),
                        transaction: None,
                        network: *network,
                    }),
                )
                    .into_response(),
            }
        }
    }
}
