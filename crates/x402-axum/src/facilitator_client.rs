//! A [`x402_rs::facilitator::Facilitator`] implementation that interacts with a _remote_ x402 Facilitator over HTTP.
//!
//! This [`FacilitatorClient`] handles the `/verify` and `/settle` endpoints of a remote facilitator,
//! and implements the [`x402_rs::facilitator::Facilitator`] trait for compatibility
//! with x402-based middleware and logic.
//!
//! ## Example
//!
//! ```rust
//! use x402_axum::facilitator_client::FacilitatorClient;
//!
//! let facilitator = FacilitatorClient::try_from("https://facilitator.ukstv.me/").unwrap();
//! ```
//! This client is cheap to clone and internally shares a connection pool via `reqwest::Client`,
//! making it safe and efficient to reuse across multiple Axum routes or concurrent tasks.
//!
//! ## Features
//!
//! - Uses `reqwest` for async HTTP requests
//! - Supports optional timeout and headers
//! - Integrates with `tracing` if the `telemetry` feature is enabled
//!
//! ## Error Handling
//!
//! Custom error types capture detailed failure contexts, including
//! - URL construction
//! - HTTP transport failures
//! - JSON deserialization errors
//! - Unexpected HTTP status responses

use http::{HeaderMap, StatusCode};
use reqwest::Client;
use std::fmt::Display;
use std::time::Duration;
use url::Url;
use x402_rs::facilitator::Facilitator;
use x402_rs::types::{
    SettleRequest, SettleResponse, SupportedPaymentKindsResponse, VerifyRequest, VerifyResponse,
};

#[cfg(feature = "telemetry")]
use tracing::{Instrument, Span};

/// A client for communicating with a remote x402 facilitator.
///
/// Handles `/verify` and `/settle` endpoints via JSON HTTP POST.
#[derive(Clone, Debug)]
pub struct FacilitatorClient {
    /// Base URL of the facilitator (e.g. `https://facilitator.example/`)
    #[allow(dead_code)] // Public for consumption by downstream crates.
    base_url: Url,
    /// Full URL to `POST /verify` requests
    verify_url: Url,
    /// Full URL to `POST /settle` requests
    settle_url: Url,
    /// Full URL to `GET /supported` requests
    supported_url: Url,
    /// Shared Reqwest HTTP client
    client: Client,
    /// Optional custom headers sent with each request
    headers: HeaderMap,
    /// Optional request timeout
    timeout: Option<Duration>,
}

impl Facilitator for FacilitatorClient {
    type Error = FacilitatorClientError;

    /// Verifies a payment payload with the facilitator.
    /// Instruments a tracing span (only when telemetry feature is enabled).
    #[cfg(feature = "telemetry")]
    async fn verify(
        &self,
        request: &VerifyRequest,
    ) -> Result<VerifyResponse, FacilitatorClientError> {
        with_span(
            FacilitatorClient::verify(self, request),
            tracing::info_span!("x402.facilitator_client.verify", timeout = ?self.timeout),
        )
        .await
    }

    /// Verifies a payment payload with the facilitator.
    /// Instruments a tracing span (only when telemetry feature is enabled).
    #[cfg(not(feature = "telemetry"))]
    async fn verify(
        &self,
        request: &VerifyRequest,
    ) -> Result<VerifyResponse, FacilitatorClientError> {
        FacilitatorClient::verify(self, request).await
    }

    /// Attempts to settle a verified payment with the facilitator.
    /// Instruments a tracing span (only when telemetry feature is enabled).
    #[cfg(feature = "telemetry")]
    async fn settle(
        &self,
        request: &SettleRequest,
    ) -> Result<SettleResponse, FacilitatorClientError> {
        with_span(
            FacilitatorClient::settle(self, request),
            tracing::info_span!("x402.facilitator_client.settle", timeout = ?self.timeout),
        )
        .await
    }

    async fn supported(&self) -> Result<SupportedPaymentKindsResponse, Self::Error> {
        FacilitatorClient::supported(self).await
    }

    /// Attempts to settle a verified payment with the facilitator.
    /// Instruments a tracing span (only when telemetry feature is enabled).
    #[cfg(not(feature = "telemetry"))]
    async fn settle(
        &self,
        request: &SettleRequest,
    ) -> Result<SettleResponse, FacilitatorClientError> {
        FacilitatorClient::settle(self, request).await
    }
}

/// Errors that can occur while interacting with a remote facilitator.
#[derive(Debug, thiserror::Error)]
pub enum FacilitatorClientError {
    #[error("URL parse error: {context}: {source}")]
    UrlParse {
        context: &'static str,
        #[source]
        source: url::ParseError,
    },
    #[error("HTTP error: {context}: {source}")]
    Http {
        context: &'static str,
        #[source]
        source: reqwest::Error,
    },
    #[error("Failed to deserialize JSON: {context}: {source}")]
    JsonDeserialization {
        context: &'static str,
        #[source]
        source: reqwest::Error,
    },
    #[error("Unexpected HTTP status {status}: {context}: {body}")]
    HttpStatus {
        context: &'static str,
        status: StatusCode,
        body: String,
    },
    #[error("Failed to read response body as text: {context}: {source}")]
    ResponseBodyRead {
        context: &'static str,
        #[source]
        source: reqwest::Error,
    },
}

impl FacilitatorClient {
    /// Returns the base URL used by this client.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Returns the computed `./verify` URL relative to [`FacilitatorClient::base_url`].
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn verify_url(&self) -> &Url {
        &self.verify_url
    }

    /// Returns the computed `./settle` URL relative to [`FacilitatorClient::base_url`]
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn settle_url(&self) -> &Url {
        &self.settle_url
    }

    /// Returns the computed `./supported` URL relative to [`FacilitatorClient::base_url`]
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn supported_url(&self) -> &Url {
        &self.supported_url
    }

    /// Returns any custom headers configured on the client.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Returns the configured timeout, if any.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn timeout(&self) -> &Option<Duration> {
        &self.timeout
    }

    /// Constructs a new [`FacilitatorClient`] from a base URL.
    ///
    /// This sets up `./verify` and `./settle` endpoint URLs relative to the base.
    pub fn try_new(base_url: Url) -> Result<Self, FacilitatorClientError> {
        let client = Client::new();
        let verify_url =
            base_url
                .join("./verify")
                .map_err(|e| FacilitatorClientError::UrlParse {
                    context: "Failed to construct ./verify URL",
                    source: e,
                })?;
        let settle_url =
            base_url
                .join("./settle")
                .map_err(|e| FacilitatorClientError::UrlParse {
                    context: "Failed to construct ./settle URL",
                    source: e,
                })?;
        let supported_url =
            base_url
                .join("./supported")
                .map_err(|e| FacilitatorClientError::UrlParse {
                    context: "Failed to construct ./supported URL",
                    source: e,
                })?;
        Ok(Self {
            client,
            base_url,
            verify_url,
            settle_url,
            supported_url,
            headers: HeaderMap::new(),
            timeout: None,
        })
    }

    /// Attaches custom headers to all future requests.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn with_headers(&self, headers: HeaderMap) -> Self {
        let mut this = self.clone();
        this.headers = headers;
        this
    }

    /// Sets a timeout for all future requests.
    #[allow(dead_code)] // Public for consumption by downstream crates.
    pub fn with_timeout(&self, timeout: Duration) -> Self {
        let mut this = self.clone();
        this.timeout = Some(timeout);
        this
    }

    /// Sends a `POST /verify` request to the facilitator.
    pub async fn verify(
        &self,
        request: &VerifyRequest,
    ) -> Result<VerifyResponse, FacilitatorClientError> {
        self.post_json(&self.verify_url, "POST /verify", request)
            .await
    }

    /// Sends a `POST /settle` request to the facilitator.
    pub async fn settle(
        &self,
        request: &SettleRequest,
    ) -> Result<SettleResponse, FacilitatorClientError> {
        self.post_json(&self.settle_url, "POST /settle", request)
            .await
    }

    pub async fn supported(&self) -> Result<SupportedPaymentKindsResponse, FacilitatorClientError> {
        self.get_json(&self.supported_url, "GET /supported").await
    }

    /// Generic POST helper that handles JSON serialization, error mapping,
    /// timeout application, and telemetry integration.
    ///
    /// `context` is a human-readable identifier used in tracing and error messages (e.g. `"POST /verify"`).
    async fn post_json<T, R>(
        &self,
        url: &Url,
        context: &'static str,
        payload: &T,
    ) -> Result<R, FacilitatorClientError>
    where
        T: serde::Serialize + ?Sized,
        R: serde::de::DeserializeOwned,
    {
        let mut req = self.client.post(url.clone()).json(payload);
        for (key, value) in self.headers.iter() {
            req = req.header(key, value);
        }
        if let Some(timeout) = self.timeout {
            req = req.timeout(timeout);
        }
        let http_response = req
            .send()
            .await
            .map_err(|e| FacilitatorClientError::Http { context, source: e })?;

        let result = if http_response.status() == StatusCode::OK {
            http_response
                .json::<R>()
                .await
                .map_err(|e| FacilitatorClientError::JsonDeserialization { context, source: e })
        } else {
            let status = http_response.status();
            let body = http_response
                .text()
                .await
                .map_err(|e| FacilitatorClientError::ResponseBodyRead { context, source: e })?;
            Err(FacilitatorClientError::HttpStatus {
                context,
                status,
                body,
            })
        };

        record_result_on_span(&result);

        result
    }

    /// Generic GET helper that handles JSON serialization, error mapping,
    /// timeout application, and telemetry integration.
    ///
    /// `context` is a human-readable identifier used in tracing and error messages (e.g. `"POST /verify"`).
    async fn get_json<R>(
        &self,
        url: &Url,
        context: &'static str,
    ) -> Result<R, FacilitatorClientError>
    where
        R: serde::de::DeserializeOwned,
    {
        let mut req = self.client.get(url.clone());
        for (key, value) in self.headers.iter() {
            req = req.header(key, value);
        }
        if let Some(timeout) = self.timeout {
            req = req.timeout(timeout);
        }
        let http_response = req
            .send()
            .await
            .map_err(|e| FacilitatorClientError::Http { context, source: e })?;

        let result = if http_response.status() == StatusCode::OK {
            http_response
                .json::<R>()
                .await
                .map_err(|e| FacilitatorClientError::JsonDeserialization { context, source: e })
        } else {
            let status = http_response.status();
            let body = http_response
                .text()
                .await
                .map_err(|e| FacilitatorClientError::ResponseBodyRead { context, source: e })?;
            Err(FacilitatorClientError::HttpStatus {
                context,
                status,
                body,
            })
        };

        record_result_on_span(&result);

        result
    }
}

/// Converts a string URL into a `FacilitatorClient`, parsing the URL and calling `try_new`.
impl TryFrom<&str> for FacilitatorClient {
    type Error = FacilitatorClientError;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        // Normalize: strip trailing slashes and add a single trailing slash
        let mut normalized = value.trim_end_matches('/').to_string();
        normalized.push('/');
        let url = Url::parse(&normalized).map_err(|e| FacilitatorClientError::UrlParse {
            context: "Failed to parse base url",
            source: e,
        })?;
        FacilitatorClient::try_new(url)
    }
}

/// Records the outcome of a request on a tracing span, including status and errors.
#[cfg(feature = "telemetry")]
fn record_result_on_span<R, E: Display>(result: &Result<R, E>) {
    let span = Span::current();
    match result {
        Ok(_) => {
            span.record("otel.status_code", "OK");
        }
        Err(err) => {
            span.record("otel.status_code", "ERROR");
            span.record("error.message", tracing::field::display(err));
            tracing::event!(tracing::Level::ERROR, error = %err, "Request to facilitator failed");
        }
    }
}

/// Records the outcome of a request on a tracing span, including status and errors.
/// Noop if telemetry feature is off.
#[cfg(not(feature = "telemetry"))]
fn record_result_on_span<R, E: Display>(_result: &Result<R, E>) {}

/// Instruments a future with a given tracing span.
#[cfg(feature = "telemetry")]
fn with_span<F: Future>(fut: F, span: Span) -> impl Future<Output = F::Output> {
    fut.instrument(span)
}
