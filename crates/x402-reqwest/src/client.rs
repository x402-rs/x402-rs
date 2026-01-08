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

/// The main x402 client that orchestrates scheme clients and selection.
pub struct X402Client<TSelector> {
    schemes: ClientSchemes,
    selector: TSelector,
}

impl X402Client<FirstMatch> {
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
    /// Register a scheme client for specific chains.
    ///
    /// # Arguments
    /// * `pattern` - Chain pattern to match (can be exact, wildcard, or set)
    /// * `scheme` - The scheme client implementation
    ///
    /// # Examples
    /// ```rust,ignore
    /// // Register for all EIP-155 chains
    /// let client = X402Client::new()
    ///     .register(ChainIdPattern::wildcard("eip155".into()), V2Eip155ExactClient::new(signer));
    ///
    /// // Register for specific chain
    /// let client = X402Client::new()
    ///     .register(ChainId::new("eip155", "84532"), V2Eip155ExactClient::new(signer));
    ///
    /// // Register for multiple chains using pattern parsing
    /// let client = X402Client::new()
    ///     .register("eip155:{1,8453,84532}".parse::<ChainIdPattern>().unwrap(), V2Eip155ExactClient::new(signer));
    /// ```
    pub fn register<S>(mut self, scheme: S) -> Self
    where
        S: X402SchemeClient + 'static,
    {
        self.schemes.push(scheme);
        self
    }

    /// Set a custom payment selector.
    #[allow(dead_code)]
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
    pub async fn make_payment_headers(&self, res: Response) -> Result<HeaderMap, X402Error> {
        let payment_required = http_payment_required_from_response(res)
            .await
            .ok_or(X402Error::ParseError("Invalid 402 response".to_string()))?;
        let candidates = self.schemes.candidates(&payment_required);

        // Select the best candidate
        let selected = self
            .selector
            .select(&candidates)
            .ok_or(X402Error::NoMatchingPaymentOption)?;

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

#[derive(Default)]
pub struct ClientSchemes(Vec<Arc<dyn X402SchemeClient>>);

impl ClientSchemes {
    pub fn push<T: X402SchemeClient + 'static>(&mut self, client: T) {
        self.0.push(Arc::new(client));
    }

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
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: rqm::Next<'_>,
    ) -> rqm::Result<Response> {
        let retry_req = req.try_clone();
        let res = next.clone().run(req, extensions).await?;
        if res.status() != StatusCode::PAYMENT_REQUIRED {
            return Ok(res);
        }

        let headers = self
            .make_payment_headers(res)
            .await
            .map_err(|e| rqm::Error::Middleware(e.into()))?;

        // Retry with payment
        let mut retry = retry_req.ok_or(rqm::Error::Middleware(
            X402Error::RequestNotCloneable.into(),
        ))?;
        retry.headers_mut().extend(headers);
        next.run(retry, extensions).await
    }
}

pub async fn http_payment_required_from_response(
    response: Response,
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

// TODO Add telemetry
// TODO Add docs
