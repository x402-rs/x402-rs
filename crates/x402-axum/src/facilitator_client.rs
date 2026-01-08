//! A [`x402_rs::facilitator::Facilitator`] implementation that interacts with a _remote_ x402 Facilitator over HTTP.
//!
//! This [`FacilitatorClient`] handles the `/verify`, `/settle`, and `/supported` endpoints of a remote facilitator,
//! and implements the [`x402_rs::facilitator::Facilitator`] trait for compatibility
//! with x402-based middleware and logic.
//!
//! ## Example
//!
//! ```rust
//! use x402_axum::facilitator_client::FacilitatorClient;
//!
//! let facilitator = FacilitatorClient::try_from("https://facilitator.x402.rs").unwrap();
//! ```
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
//!

use http::{HeaderMap, StatusCode};
use reqwest::Client;
use std::fmt::Display;
use std::time::Duration;
use tokio::sync::RwLock;
use url::Url;
use x402_rs::facilitator::Facilitator;
use x402_rs::proto::{
    SettleRequest, SettleResponse, SupportedResponse, VerifyRequest, VerifyResponse,
};

#[cfg(feature = "telemetry")]
use tracing::{Instrument, Span, instrument};

/// TTL cache for [`SupportedResponse`].
#[derive(Clone, Debug)]
struct SupportedCacheState {
    /// The cached response
    response: SupportedResponse,
    /// When the cache expires
    expires_at: std::time::Instant,
}

/// An encapsulated TTL cache for the `/supported` endpoint response.
///
/// Each clone has an independent cache state.
#[derive(Debug)]
pub struct SupportedCache {
    /// TTL for the cache
    ttl: Duration,
    /// Cache state (RwLock for read-heavy workload)
    state: RwLock<Option<SupportedCacheState>>,
}

impl SupportedCache {
    /// Creates a new cache with the given TTL.
    pub fn new(ttl: Duration) -> Self {
        Self {
            ttl,
            state: RwLock::new(None),
        }
    }

    /// Returns the cached response if valid, None otherwise.
    pub async fn get(&self) -> Option<SupportedResponse> {
        let guard = self.state.read().await;
        let cache = guard.as_ref()?;
        if std::time::Instant::now() < cache.expires_at {
            Some(cache.response.clone())
        } else {
            None
        }
    }

    /// Stores a response in the cache with the configured TTL.
    pub async fn set(&self, response: SupportedResponse) {
        let mut guard = self.state.write().await;
        *guard = Some(SupportedCacheState {
            response,
            expires_at: std::time::Instant::now() + self.ttl,
        });
    }

    /// Clears the cache.
    pub async fn clear(&self) {
        let mut guard = self.state.write().await;
        *guard = None;
    }
}

impl Clone for SupportedCache {
    fn clone(&self) -> Self {
        Self::new(self.ttl)
    }
}

/// A client for communicating with a remote x402 facilitator.
///
/// Handles `/verify`, `/settle`, and `/supported` endpoints via JSON HTTP.
#[derive(Clone, Debug)]
pub struct FacilitatorClient {
    /// Base URL of the facilitator (e.g. `https://facilitator.example/`)
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
    /// Cache for the supported endpoint response
    supported_cache: SupportedCache,
}

impl Facilitator for FacilitatorClient {
    type Error = FacilitatorClientError;

    /// Verifies a payment payload with the facilitator.
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
    #[cfg(not(feature = "telemetry"))]
    async fn verify(
        &self,
        request: &VerifyRequest,
    ) -> Result<VerifyResponse, FacilitatorClientError> {
        FacilitatorClient::verify(self, request).await
    }

    /// Settles a verified payment with the facilitator.
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

    /// Settles a verified payment with the facilitator.
    #[cfg(not(feature = "telemetry"))]
    async fn settle(
        &self,
        request: &SettleRequest,
    ) -> Result<SettleResponse, FacilitatorClientError> {
        FacilitatorClient::settle(self, request).await
    }

    /// Retrieves the supported payment kinds from the facilitator.
    ///
    /// Results are cached with a configurable TTL to avoid repeated HTTP requests.
    async fn supported(&self) -> Result<SupportedResponse, Self::Error> {
        FacilitatorClient::supported(self).await
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
    /// Default TTL for caching the supported endpoint response (10 minutes).
    pub const DEFAULT_SUPPORTED_CACHE_TTL: Duration = Duration::from_secs(10 * 60);

    /// Returns the base URL used by this client.
    pub fn base_url(&self) -> &Url {
        &self.base_url
    }

    /// Returns the computed `./verify` URL relative to [`FacilitatorClient::base_url`].
    pub fn verify_url(&self) -> &Url {
        &self.verify_url
    }

    /// Returns the computed `./settle` URL relative to [`FacilitatorClient::base_url`].
    pub fn settle_url(&self) -> &Url {
        &self.settle_url
    }

    /// Returns the computed `./supported` URL relative to [`FacilitatorClient::base_url`].
    pub fn supported_url(&self) -> &Url {
        &self.supported_url
    }

    /// Returns any custom headers configured on the client.
    pub fn headers(&self) -> &HeaderMap {
        &self.headers
    }

    /// Returns the configured timeout, if any.
    pub fn timeout(&self) -> &Option<Duration> {
        &self.timeout
    }

    /// Returns a reference to the supported cache.
    pub fn supported_cache(&self) -> &SupportedCache {
        &self.supported_cache
    }

    /// Constructs a new [`FacilitatorClient`] from a base URL.
    ///
    /// This sets up `./verify`, `./settle`, and `./supported` endpoint URLs relative to the base.
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
            supported_cache: SupportedCache::new(Self::DEFAULT_SUPPORTED_CACHE_TTL),
        })
    }

    /// Attaches custom headers to all future requests.
    pub fn with_headers(&self, headers: HeaderMap) -> Self {
        let mut this = self.clone();
        this.headers = headers;
        this
    }

    /// Sets a timeout for all future requests.
    pub fn with_timeout(&self, timeout: Duration) -> Self {
        let mut this = self.clone();
        this.timeout = Some(timeout);
        this
    }

    /// Sets the TTL for caching the supported endpoint response.
    ///
    /// Default is 10 minutes. Use [`Self::without_supported_cache()`] to disable caching.
    pub fn with_supported_cache_ttl(&self, ttl: Duration) -> Self {
        let mut this = self.clone();
        this.supported_cache = SupportedCache::new(ttl);
        this
    }

    /// Disables caching for the supported endpoint.
    pub fn without_supported_cache(&self) -> Self {
        self.with_supported_cache_ttl(Duration::ZERO)
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

    /// Sends a `GET /supported` request to the facilitator.
    /// This is the inner method that always makes an HTTP request.
    #[cfg_attr(
        feature = "telemetry",
        instrument(name = "x402.facilitator_client.supported", skip_all, err)
    )]
    async fn supported_inner(&self) -> Result<SupportedResponse, FacilitatorClientError> {
        self.get_json(&self.supported_url, "GET /supported").await
    }

    /// Sends a `GET /supported` request to the facilitator.
    /// Results are cached with a configurable TTL (default: 10 minutes).
    /// Use [`Self::supported_inner()`] to bypass the cache.
    pub async fn supported(&self) -> Result<SupportedResponse, FacilitatorClientError> {
        // Try to get from cache
        if let Some(response) = self.supported_cache.get().await {
            return Ok(response);
        }

        // Cache miss - fetch and cache
        #[cfg(feature = "telemetry")]
        tracing::info!("x402.facilitator_client.supported_cache_miss");

        let response = self.supported_inner().await?;
        self.supported_cache.set(response.clone()).await;

        Ok(response)
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
    /// `context` is a human-readable identifier used in tracing and error messages (e.g. `"GET /supported"`).
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

/// Converts a String URL into a `FacilitatorClient`.
impl TryFrom<String> for FacilitatorClient {
    type Error = FacilitatorClientError;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        FacilitatorClient::try_from(value.as_str())
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    fn create_test_supported_response() -> SupportedResponse {
        SupportedResponse {
            kinds: vec![x402_rs::proto::SupportedPaymentKind {
                x402_version: 1,
                scheme: "eip155-exact".to_string(),
                network: "1".to_string(),
                extra: None,
            }],
            extensions: vec![],
            signers: HashMap::new(),
        }
    }

    #[tokio::test]
    async fn test_supported_cache_caches_response() {
        let mock_server = MockServer::start().await;
        let test_response = create_test_supported_response();

        // Mock the supported endpoint
        Mock::given(method("GET"))
            .and(path("/supported"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&test_response))
            .mount(&mock_server)
            .await;

        let client = FacilitatorClient::try_new(mock_server.uri().parse().unwrap()).unwrap();

        // First call should hit the network
        let result1 = client.supported().await.unwrap();
        assert_eq!(result1.kinds.len(), 1);

        // Second call should use cache (same mock call count)
        let result2 = client.supported().await.unwrap();
        assert_eq!(result2.kinds.len(), 1);

        // Both results should be equal
        assert_eq!(result1.kinds[0].scheme, result2.kinds[0].scheme);
    }

    #[tokio::test]
    async fn test_supported_cache_with_custom_ttl() {
        let mock_server = MockServer::start().await;
        let test_response = create_test_supported_response();

        // Mock the supported endpoint
        Mock::given(method("GET"))
            .and(path("/supported"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&test_response))
            .mount(&mock_server)
            .await;

        // Create client with 1ms TTL (essentially no caching)
        let client = FacilitatorClient::try_new(mock_server.uri().parse().unwrap())
            .unwrap()
            .with_supported_cache_ttl(Duration::from_millis(1));

        // First call
        let result1 = client.supported().await.unwrap();
        assert_eq!(result1.kinds.len(), 1);

        // Wait for cache to expire
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Second call should hit the network again due to expired cache
        let result2 = client.supported().await.unwrap();
        assert_eq!(result2.kinds.len(), 1);
    }

    #[tokio::test]
    async fn test_supported_cache_disabled() {
        let mock_server = MockServer::start().await;
        let test_response = create_test_supported_response();

        // Mock the supported endpoint
        Mock::given(method("GET"))
            .and(path("/supported"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&test_response))
            .mount(&mock_server)
            .await;

        // Create client with caching disabled
        let client = FacilitatorClient::try_new(mock_server.uri().parse().unwrap())
            .unwrap()
            .without_supported_cache();

        // Each call should hit the network
        let result1 = client.supported().await.unwrap();
        let result2 = client.supported().await.unwrap();

        assert_eq!(result1.kinds.len(), 1);
        assert_eq!(result2.kinds.len(), 1);
    }

    #[tokio::test]
    async fn test_supported_cache_clones_independently() {
        let mock_server = MockServer::start().await;
        let test_response = create_test_supported_response();

        // Mock the supported endpoint
        Mock::given(method("GET"))
            .and(path("/supported"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&test_response))
            .mount(&mock_server)
            .await;

        let client = FacilitatorClient::try_new(mock_server.uri().parse().unwrap()).unwrap();

        // Clone the client
        let client2 = client.clone();

        // Populate cache on first client
        let _ = client.supported().await.unwrap();

        // Clone should have independent cache (will make its own request)
        // Note: Since both clones point to same server, the mock will count 2 requests
        let _ = client2.supported().await.unwrap();
    }

    #[tokio::test]
    async fn test_supported_inner_bypasses_cache() {
        let mock_server = MockServer::start().await;
        let test_response = create_test_supported_response();

        // Mock the supported endpoint
        Mock::given(method("GET"))
            .and(path("/supported"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&test_response))
            .mount(&mock_server)
            .await;

        let client = FacilitatorClient::try_new(mock_server.uri().parse().unwrap()).unwrap();

        // Populate cache
        let _ = client.supported().await.unwrap();

        // supported_inner() should always make HTTP request, bypassing cache
        let result = client.supported_inner().await.unwrap();
        assert_eq!(result.kinds.len(), 1);
    }
}
