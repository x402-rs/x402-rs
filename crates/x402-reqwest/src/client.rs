//! Client-side x402 payment handling for reqwest.
//!
//! This module provides the [`X402Client`] which orchestrates scheme clients
//! and payment selection for automatic payment handling.

use http::{Extensions, HeaderMap, StatusCode};
use reqwest::{Request, Response};
use reqwest_middleware as rqm;
use std::sync::Arc;
use x402_rs::proto;
use x402_rs::proto::client::{
    FirstMatch, PaymentCandidate, PaymentSelector, X402Error, X402SchemeClient,
};
use x402_rs::proto::{v1, v2};
use x402_rs::util::Base64Bytes;

#[cfg(feature = "telemetry")]
use tracing::{debug, info, instrument, trace};

/// The main x402 client that orchestrates scheme clients and selection.
///
/// The [`X402Client`] acts as middleware for reqwest, automatically handling
/// 402 Payment Required responses by extracting payment requirements, signing
/// payments, and retrying requests.
///
/// ## Creating an X402Client
///
/// ```rust,no_run
/// use x402_reqwest::X402Client;
///
/// let client = X402Client::new();
/// ```
///
/// ## Registering Scheme Clients
///
/// To handle payments on different chains, register scheme clients:
///
/// ```rust,no_run
/// use x402_reqwest::X402Client;
/// use x402_rs::scheme::v1_eip155_exact::client::V1Eip155ExactClient;
/// use alloy_signer_local::PrivateKeySigner;
/// use std::sync::Arc;
///
/// let signer = Arc::new("PRIVATE_KEY".parse::<PrivateKeySigner>().unwrap());
/// let client = X402Client::new()
///     .register(V1Eip155ExactClient::new(signer));
/// ```
///
/// ## Using with Reqwest
///
/// See the [`ReqwestWithPayments`] trait for integrating with reqwest.
pub struct X402Client<TSelector> {
    schemes: ClientSchemes,
    selector: TSelector,
}

impl X402Client<FirstMatch> {
    /// Creates a new [`X402Client`] with default settings.
    ///
    /// The default client uses [`FirstMatch`] payment selection, which selects
    /// the first matching payment scheme.
    pub fn new() -> Self {
        Self::default()
    }
}

impl Default for X402Client<FirstMatch> {
    fn default() -> Self {
        Self {
            schemes: ClientSchemes::default(),
            selector: FirstMatch,
        }
    }
}

impl<TSelector> X402Client<TSelector> {
    /// Registers a scheme client for specific chains or networks.
    ///
    /// Scheme clients handle the actual payment signing for specific protocols.
    /// You can register multiple clients for different chains or schemes.
    ///
    /// # Arguments
    ///
    /// * `scheme` - The scheme client implementation to register
    ///
    /// # Returns
    ///
    /// A new [`X402Client`] with the additional scheme registered.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// use x402_reqwest::X402Client;
    /// use x402_rs::scheme::v1_eip155_exact::client::V1Eip155ExactClient;
    /// use alloy_signer_local::PrivateKeySigner;
    /// use std::sync::Arc;
    ///
    /// let signer = Arc::new("PRIVATE_KEY".parse::<PrivateKeySigner>().unwrap());
    /// let client = X402Client::new()
    ///     .register(V1Eip155ExactClient::new(signer));
    /// ```
    pub fn register<S>(mut self, scheme: S) -> Self
    where
        S: X402SchemeClient + 'static,
    {
        self.schemes.push(scheme);
        self
    }

    /// Sets a custom payment selector.
    ///
    /// By default, [`FirstMatch`] is used which selects the first matching scheme.
    /// You can implement custom selection logic by providing your own [`PaymentSelector`].
    ///
    /// # Examples
    ///
    /// ```rust,ignore
    /// use x402_reqwest::X402Client;
    /// use x402_rs::proto::client::{FirstMatch, PaymentSelector};
    ///
    /// let client = X402Client::new()
    ///     .with_selector(MyCustomSelector);
    /// ```
    pub fn with_selector<P: PaymentSelector + 'static>(self, selector: P) -> X402Client<P> {
        X402Client {
            selector,
            schemes: self.schemes,
        }
    }
}

impl<TSelector> X402Client<TSelector>
where
    TSelector: PaymentSelector,
{
    /// Creates payment headers from a 402 response.
    ///
    /// This method extracts the payment requirements from the response,
    /// selects the best payment option, signs the payment, and returns
    /// the appropriate headers to include in the retry request.
    ///
    /// # Arguments
    ///
    /// * `res` - The 402 Payment Required response
    ///
    /// # Returns
    ///
    /// A [`HeaderMap`] containing the payment signature header, or an error.
    ///
    /// # Errors
    ///
    /// Returns [`X402Error::ParseError`] if the response cannot be parsed.
    /// Returns [`X402Error::NoMatchingPaymentOption`] if no registered scheme
    /// can handle the payment requirements.
    #[cfg_attr(feature = "telemetry", instrument(name = "x402.reqwest.make_payment_headers", skip_all, err))]
    pub async fn make_payment_headers(&self, res: Response) -> Result<HeaderMap, X402Error> {
        let payment_required = parse_payment_required(res)
            .await
            .ok_or(X402Error::ParseError("Invalid 402 response".to_string()))?;
        let candidates = self.schemes.candidates(&payment_required);

        // Select the best candidate
        let selected = self
            .selector
            .select(&candidates)
            .ok_or(X402Error::NoMatchingPaymentOption)?;

        #[cfg(feature = "telemetry")]
        debug!(
            scheme = %selected.scheme,
            chain_id = %selected.chain_id,
            "Selected payment scheme"
        );

        let signed_payload = selected.sign().await?;
        let header_name = match &payment_required {
            proto::PaymentRequired::V1(_) => "X-Payment",
            proto::PaymentRequired::V2(_) => "Payment-Signature",
        };
        let headers = {
            let mut headers = HeaderMap::new();
            headers.insert(header_name, signed_payload.parse().unwrap());
            headers
        };

        Ok(headers)
    }
}

/// Internal collection of registered scheme clients.
#[derive(Default)]
pub struct ClientSchemes(Vec<Arc<dyn X402SchemeClient>>);

impl ClientSchemes {
    /// Adds a scheme client to the collection.
    pub fn push<T: X402SchemeClient + 'static>(&mut self, client: T) {
        self.0.push(Arc::new(client));
    }

    /// Finds all payment candidates that can handle the given payment requirements.
    pub fn candidates(&self, payment_required: &proto::PaymentRequired) -> Vec<PaymentCandidate> {
        let mut candidates = vec![];
        for client in self.0.iter() {
            let accepted = client.accept(payment_required);
            candidates.extend(accepted);
        }
        candidates
    }
}

#[async_trait::async_trait]
impl<TSelector> rqm::Middleware for X402Client<TSelector>
where
    TSelector: PaymentSelector + Send + Sync + 'static,
{
    /// Handles a request, automatically handling 402 responses.
    ///
    /// When a 402 response is received, this middleware:
    /// 1. Extracts payment requirements from the response
    /// 2. Signs a payment using registered scheme clients
    /// 3. Retries the request with the payment header
    #[cfg_attr(feature = "telemetry", instrument(name = "x402.reqwest.handle", skip_all, err))]
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: rqm::Next<'_>,
    ) -> rqm::Result<Response> {
        let retry_req = req.try_clone();
        let res = next.clone().run(req, extensions).await?;

        if res.status() != StatusCode::PAYMENT_REQUIRED {
            #[cfg(feature = "telemetry")]
            trace!(status = ?res.status(), "No payment required, returning response");
            return Ok(res);
        }

        #[cfg(feature = "telemetry")]
        info!(url = ?res.url(), "Received 402 Payment Required, processing payment");

        let headers = self
            .make_payment_headers(res)
            .await
            .map_err(|e| rqm::Error::Middleware(e.into()))?;

        // Retry with payment
        let mut retry = retry_req.ok_or(rqm::Error::Middleware(
            X402Error::RequestNotCloneable.into(),
        ))?;
        retry.headers_mut().extend(headers);

        #[cfg(feature = "telemetry")]
        trace!(url = ?retry.url(), "Retrying request with payment headers");

        next.run(retry, extensions).await
    }
}

/// Parses a 402 Payment Required response into a [`proto::PaymentRequired`].
///
/// Supports both V1 (JSON body) and V2 (base64-encoded header) formats.
#[cfg_attr(feature = "telemetry", instrument(name = "x402.reqwest.parse_payment_required", skip(response)))]
pub async fn parse_payment_required(
    response: Response,
) -> Option<proto::PaymentRequired> {
    // Try V2 format first (header-based)
    let headers = response.headers();
    let v2_payment_required = headers
        .get("Payment-Required")
        .and_then(|h| Base64Bytes::from(h.as_bytes()).decode().ok())
        .and_then(|b| serde_json::from_slice::<v2::PaymentRequired>(&b).ok());
    if let Some(v2_payment_required) = v2_payment_required {
        #[cfg(feature = "telemetry")]
        debug!("Parsed V2 payment required from header");
        return Some(proto::PaymentRequired::V2(v2_payment_required));
    }

    // Fall back to V1 format (body-based)
    let v1_payment_required = response
        .bytes()
        .await
        .ok()
        .and_then(|b| serde_json::from_slice::<v1::PaymentRequired>(&b).ok());
    if let Some(v1_payment_required) = v1_payment_required {
        #[cfg(feature = "telemetry")]
        debug!("Parsed V1 payment required from body");
        return Some(proto::PaymentRequired::V1(v1_payment_required));
    }

    #[cfg(feature = "telemetry")]
    debug!("Could not parse payment required from response");

    None
}
