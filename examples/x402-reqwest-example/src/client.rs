use std::error::Error;
use http::Extensions;
use reqwest::{Client, ClientBuilder, Request, Response, StatusCode};
use reqwest_middleware as rqm;
use std::sync::Arc;
use x402_rs::chain::{ChainId, ChainIdPattern};
use x402_rs::proto;
use x402_rs::proto::client::{FirstMatch, PaymentCandidate, PaymentSelector, X402Error};
use x402_rs::scheme::X402SchemeId;

use crate::http_transport::HttpPaymentRequired;

/// The main x402 client that orchestrates scheme clients and selection.
pub struct X402Client<TSelector> {
    schemes: ClientSchemes,
    selector: TSelector,
}

impl X402Client<FirstMatch> {
    pub fn new() -> Self {
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
    pub fn register<P, S>(mut self, pattern: P, scheme: S) -> Self
    where
        P: Into<ChainIdPattern>,
        S: X402SchemeClient + 'static,
    {
        self.schemes.push(RegisteredSchemeClient {
            pattern: pattern.into(),
            client: Arc::new(scheme),
        });
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

impl<TSelector> X402Client<TSelector> where TSelector: PaymentSelector {
    pub async fn make_payment_header(&self, res: Response) -> Result<(), X402Error> {
        let payment_quote = HttpPaymentRequired::from_response(res)
                .await
                .ok_or(X402Error::ParseError("Invalid 402 response".to_string()))?;
            let candidates = self.schemes.candidates(&payment_quote);

            println!("Found {} candidates", candidates.len());
            for (i, c) in candidates.iter().enumerate() {
                println!(
                    "  [{}] chain={}, asset={}, amount={}",
                    i,
                    c.chain_id,
                    c.asset,
                    c.amount
                );
            }

            // Select the best candidate
            let selected = self
                .selector
                .select(&candidates)
                .ok_or(X402Error::NoMatchingPaymentOption)?;

            println!("selected {:?} {:?}", selected.chain_id, selected.amount);
            let s = selected.sign().await;
            println!("signed {:?}", s);

        Ok(())
    }
}

#[derive(Default)]
pub struct ClientSchemes(Vec<RegisteredSchemeClient>);

impl ClientSchemes {
    pub fn push(&mut self, client: RegisteredSchemeClient) {
        self.0.push(client);
    }

    pub fn iter(&self) -> impl Iterator<Item = &RegisteredSchemeClient> {
        self.0.iter()
    }

    pub fn candidates(
        &self,
        payment_quote: &HttpPaymentRequired,
    ) -> Vec<PaymentCandidate> {
        let mut candidates = vec![];
        for scheme_client in self.0.iter() {
            let client = scheme_client.client();
            let req = payment_quote.as_payment_required();
            let accepted = client.accept(req);
            candidates.extend(accepted);
        }
        candidates
    }
}

/// Internal wrapper that pairs a scheme client with its chain pattern.
pub struct RegisteredSchemeClient {
    pattern: ChainIdPattern,
    client: Arc<dyn X402SchemeClient>,
}

impl RegisteredSchemeClient {
    /// Check if this registered client can handle the given payment requirement.
    ///
    /// Matching logic:
    /// 1. x402_version must match
    /// 2. scheme name must match
    /// 3. namespace from X402SchemeId must match chain_id namespace
    /// 4. pattern must match the chain_id (for reference matching)
    pub fn matches(&self, version: u8, scheme: &str, chain_id: &ChainId) -> bool {
        self.client.x402_version() == version
            && self.client.scheme() == scheme
            && self.client.namespace() == chain_id.namespace()
            && self.pattern.matches(chain_id)
    }

    pub fn client(&self) -> &dyn X402SchemeClient {
        self.client.as_ref()
    }
}

#[async_trait::async_trait]
pub trait X402SchemeClient: X402SchemeId + Send + Sync {
    fn accept(
        &self,
        payment_required: &proto::PaymentRequired,
    ) -> Vec<PaymentCandidate>;
}

pub trait ReqwestWithPayments<A, S> {
    fn with_payments(self, x402_client: X402Client<S>) -> ReqwestWithPaymentsBuilder<A, S>;
}

impl<S> ReqwestWithPayments<Client, S> for Client {
    fn with_payments(self, x402_client: X402Client<S>) -> ReqwestWithPaymentsBuilder<Client, S> {
        ReqwestWithPaymentsBuilder {
            inner: self,
            x402_client,
        }
    }
}

impl<S> ReqwestWithPayments<ClientBuilder, S> for ClientBuilder {
    fn with_payments(
        self,
        x402_client: X402Client<S>,
    ) -> ReqwestWithPaymentsBuilder<ClientBuilder, S> {
        ReqwestWithPaymentsBuilder {
            inner: self,
            x402_client,
        }
    }
}

pub struct ReqwestWithPaymentsBuilder<A, S> {
    inner: A,
    x402_client: X402Client<S>,
}

pub trait ReqwestWithPaymentsBuild {
    type BuildResult;
    type BuilderResult;

    fn build(self) -> Self::BuildResult;
    fn builder(self) -> Self::BuilderResult;
}

impl<S> ReqwestWithPaymentsBuild for ReqwestWithPaymentsBuilder<Client, S>
where
    X402Client<S>: rqm::Middleware,
{
    type BuildResult = rqm::ClientWithMiddleware;
    type BuilderResult = rqm::ClientBuilder;

    fn build(self) -> Self::BuildResult {
        self.builder().build()
    }

    fn builder(self) -> Self::BuilderResult {
        rqm::ClientBuilder::new(self.inner).with(self.x402_client)
    }
}

impl<S> ReqwestWithPaymentsBuild for ReqwestWithPaymentsBuilder<ClientBuilder, S>
where
    X402Client<S>: rqm::Middleware,
{
    type BuildResult = Result<rqm::ClientWithMiddleware, reqwest::Error>;
    type BuilderResult = Result<rqm::ClientBuilder, reqwest::Error>;

    fn build(self) -> Self::BuildResult {
        let builder = self.builder()?;
        Ok(builder.build())
    }

    fn builder(self) -> Self::BuilderResult {
        let client = self.inner.build()?;
        Ok(rqm::ClientBuilder::new(client).with(self.x402_client))
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

        self.make_payment_header(res).await.map_err(|e| rqm::Error::Middleware(e.into()))?;

        // // Build candidates from the 402 response
        // let (candidates, _version) = self
        //     .build_candidates(res).await
        //     .map_err(Into::<rqm::Error>::into)?;
        //
        // println!("Found {} candidates", candidates.len());
        // for (i, c) in candidates.iter().enumerate() {
        //     println!(
        //         "  [{}] chain={}, asset={}, amount={}",
        //         i, c.chain_id, c.asset, c.amount
        //     );
        // }
        //
        // // Select the best candidate
        // let selected = self
        //     .selector
        //     .select(&candidates)
        //     .ok_or(X402Error::NoMatchingPaymentOption)?;
        //
        // println!(
        //     "Selected candidate: chain={}, amount={}",
        //     selected.chain_id, selected.amount
        // );
        //
        // // Sign the payment using the client reference stored in the candidate
        // let payment_header = selected
        //     .client
        //     .sign_payment(selected)
        //     .await
        //     .map_err(Into::<rqm::Error>::into)?;
        //
        // println!("Payment header length: {} bytes", payment_header.len());
        //
        // // Retry with payment
        let mut retry = retry_req.ok_or(rqm::Error::Middleware(X402Error::RequestNotCloneable.into()))?;
        // retry.headers_mut().insert(
        //     "PAYMENT-SIGNATURE",
        //     payment_header
        //         .parse()
        //         .map_err(|e| X402Error::SigningError(format!("{e}")))?,
        // );

        next.run(retry, extensions).await
    }
}
