use axum::http::StatusCode;
use axum::{response::IntoResponse, Extension, Json};
use serde_json::json;
use std::sync::Arc;
use tracing::instrument;

use crate::facilitator::{settle, verify, PaymentError};
use crate::provider_cache::ProviderCache;
use crate::types::{
    ErrorReason, ErrorResponse, SettleRequest, SettleResponse, VerifyRequest, VerifyResponse,
};

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

#[instrument(skip_all)]
pub async fn get_supported() -> impl IntoResponse {
    Json(json!({
        "kinds": [
            {
                "x402Version": 1,
                "scheme": "exact",
                "network": "base-sepolia",
            }
        ]
    }))
}

#[instrument(skip_all)]
pub async fn post_verify(
    Extension(provider_cache): Extension<Arc<ProviderCache>>,
    Json(body): Json<VerifyRequest>,
) -> impl IntoResponse {
    let payload = &body.payment_payload;
    let payment_requirements = &body.payment_requirements;
    let payer = &payload.payload.authorization.from;

    match verify(provider_cache, payload, payment_requirements).await {
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
                Json(VerifyResponse {
                    is_valid: false,
                    invalid_reason: Some(ErrorReason::InvalidScheme),
                    payer: *payer,
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
                    StatusCode::OK,
                    Json(VerifyResponse {
                        is_valid: false,
                        invalid_reason: Some(ErrorReason::InsufficientFunds),
                        payer: *payer,
                    }),
                )
                    .into_response(),
            }
        }
    }
}

#[instrument(skip_all)]
pub async fn post_settle(
    Extension(provider_cache): Extension<Arc<ProviderCache>>,
    Json(body): Json<SettleRequest>,
) -> impl IntoResponse {
    let payment_payload = &body.payment_payload;
    let payment_requirements = &body.payment_requirements;
    let payer = &payment_payload.payload.authorization.from;
    let network = &body.payment_payload.network;
    match settle(provider_cache, payment_payload, payment_requirements).await {
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
                    error_reason: Some(ErrorReason::InvalidScheme),
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
                        error_reason: Some(ErrorReason::InsufficientFunds),
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
