//! Prometheus metrics for the x402 facilitator.
//!
//! Exposes request counters and latency histograms for payment verification and
//! settlement operations. All metrics are served at `GET /metrics` in Prometheus
//! text exposition format.
//!
//! # Label safety
//!
//! The `scheme` and `chain` labels are bounded by the facilitator's configured
//! scheme registry — only values that pass [`SchemeHandlerSlug`] parsing from a
//! valid request body are recorded. Requests with unparseable payloads are
//! recorded with `scheme="unknown"` and `chain="unknown"`, preventing unbounded
//! label cardinality from malicious input.

use axum::http::StatusCode;
use axum::response::IntoResponse;
use prometheus::{
    Encoder, HistogramOpts, HistogramVec, IntCounterVec, Opts, Registry, TextEncoder,
};
use std::sync::LazyLock;

/// Isolated Prometheus registry for facilitator metrics.
///
/// Uses a dedicated registry (not the global default) to avoid naming conflicts
/// when the facilitator is embedded alongside other instrumented components.
static REGISTRY: LazyLock<Registry> = LazyLock::new(Registry::new);

/// Total verify requests, labelled by outcome, scheme, and chain.
///
/// The `status` label uses one of: `ok`, `client_error` (HTTP 400/412),
/// `server_error` (HTTP 500), or `invalid_request` (unparseable body).
static VERIFY_REQUESTS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        "x402_facilitator_verify_requests_total",
        "Total payment verification requests",
    );
    let counter = IntCounterVec::new(opts, &["status", "scheme", "chain"])
        .expect("failed to create verify_requests metric");
    REGISTRY
        .register(Box::new(counter.clone()))
        .expect("failed to register verify_requests metric");
    counter
});

/// Total settle requests, labelled by outcome, scheme, and chain.
static SETTLE_REQUESTS: LazyLock<IntCounterVec> = LazyLock::new(|| {
    let opts = Opts::new(
        "x402_facilitator_settle_requests_total",
        "Total payment settlement requests",
    );
    let counter = IntCounterVec::new(opts, &["status", "scheme", "chain"])
        .expect("failed to create settle_requests metric");
    REGISTRY
        .register(Box::new(counter.clone()))
        .expect("failed to register settle_requests metric");
    counter
});

/// Verify request duration in seconds.
static VERIFY_DURATION: LazyLock<HistogramVec> = LazyLock::new(|| {
    let opts = HistogramOpts::new(
        "x402_facilitator_verify_duration_seconds",
        "Payment verification latency in seconds",
    )
    .buckets(vec![
        0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
    ]);
    let histogram = HistogramVec::new(opts, &["scheme", "chain"])
        .expect("failed to create verify_duration metric");
    REGISTRY
        .register(Box::new(histogram.clone()))
        .expect("failed to register verify_duration metric");
    histogram
});

/// Settle request duration in seconds.
static SETTLE_DURATION: LazyLock<HistogramVec> = LazyLock::new(|| {
    let opts = HistogramOpts::new(
        "x402_facilitator_settle_duration_seconds",
        "Payment settlement latency in seconds",
    )
    .buckets(vec![
        0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0, 30.0, 60.0,
    ]);
    let histogram = HistogramVec::new(opts, &["scheme", "chain"])
        .expect("failed to create settle_duration metric");
    REGISTRY
        .register(Box::new(histogram.clone()))
        .expect("failed to register settle_duration metric");
    histogram
});

/// Record a verify request with its outcome, labels, and duration.
pub fn record_verify(status: &str, scheme: &str, chain: &str, duration_secs: f64) {
    VERIFY_REQUESTS
        .with_label_values(&[status, scheme, chain])
        .inc();
    VERIFY_DURATION
        .with_label_values(&[scheme, chain])
        .observe(duration_secs);
}

/// Record a settle request with its outcome, labels, and duration.
pub fn record_settle(status: &str, scheme: &str, chain: &str, duration_secs: f64) {
    SETTLE_REQUESTS
        .with_label_values(&[status, scheme, chain])
        .inc();
    SETTLE_DURATION
        .with_label_values(&[scheme, chain])
        .observe(duration_secs);
}

/// `GET /metrics`: Prometheus text exposition endpoint.
pub async fn get_metrics() -> impl IntoResponse {
    // Force lazy initialization so metrics appear even before the first request.
    let _ = &*VERIFY_REQUESTS;
    let _ = &*SETTLE_REQUESTS;
    let _ = &*VERIFY_DURATION;
    let _ = &*SETTLE_DURATION;

    let encoder = TextEncoder::new();
    let metric_families = REGISTRY.gather();
    let mut buffer = Vec::new();
    match encoder.encode(&metric_families, &mut buffer) {
        Ok(()) => (
            StatusCode::OK,
            [(axum::http::header::CONTENT_TYPE, encoder.format_type())],
            buffer,
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("metrics encoding error: {e}"),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_record_verify_increments_counter() {
        record_verify("ok", "exact", "eip155:8453", 0.042);
        record_verify("client_error", "exact", "eip155:8453", 0.003);

        let families = REGISTRY.gather();
        let verify_family = families
            .iter()
            .find(|f| f.get_name() == "x402_facilitator_verify_requests_total")
            .expect("verify counter not found");

        let total: f64 = verify_family
            .get_metric()
            .iter()
            .map(|m| m.get_counter().get_value())
            .sum();
        assert!(total >= 2.0, "expected at least 2 verify requests, got {total}");
    }

    #[test]
    fn test_record_settle_increments_counter() {
        record_settle("ok", "exact", "eip155:8453", 1.5);
        record_settle("server_error", "exact", "eip155:8453", 0.8);

        let families = REGISTRY.gather();
        let settle_family = families
            .iter()
            .find(|f| f.get_name() == "x402_facilitator_settle_requests_total")
            .expect("settle counter not found");

        let total: f64 = settle_family
            .get_metric()
            .iter()
            .map(|m| m.get_counter().get_value())
            .sum();
        assert!(total >= 2.0, "expected at least 2 settle requests, got {total}");
    }

    #[tokio::test]
    async fn test_metrics_endpoint_returns_valid_prometheus_format() {
        // Record something so metrics are non-empty.
        record_verify("ok", "exact", "eip155:84532", 0.01);

        let response = get_metrics().await.into_response();
        assert_eq!(response.status(), StatusCode::OK);

        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let text = String::from_utf8(body.to_vec()).unwrap();

        assert!(
            text.contains("x402_facilitator_verify_requests_total"),
            "missing verify counter in output"
        );
        assert!(
            text.contains("x402_facilitator_verify_duration_seconds"),
            "missing verify histogram in output"
        );
        assert!(
            text.contains("x402_facilitator_settle_requests_total"),
            "missing settle counter in output"
        );
    }

    #[test]
    fn test_unknown_scheme_chain_does_not_panic() {
        // Simulates an unparseable request — should not panic.
        record_verify("invalid_request", "unknown", "unknown", 0.001);
        record_settle("invalid_request", "unknown", "unknown", 0.001);
    }
}
