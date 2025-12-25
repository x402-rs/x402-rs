use std::sync::Arc;
use http::Extensions;
use reqwest::{Client, ClientBuilder, Request, Response, StatusCode};
use reqwest_middleware as rqm;
use reqwest_middleware::{ClientWithMiddleware, Next};
use x402_rs::chain::ChainId;
use x402_rs::proto;
use x402_rs::util::b64::Base64Bytes;

pub struct X402Client {
    schemes: Vec<Arc<dyn X402SchemeClient + Send + Sync>>,
}

impl X402Client {
    pub fn new() -> Self {
        Self {
            schemes: vec![],
        }
    }

    pub fn register<S: X402SchemeClient + Send + Sync + 'static>(mut self, scheme: S) -> Self {
        self.schemes.push(Arc::new(scheme));
        self
    }
}

pub trait ReqwestWithPayments<A> {
    fn with_payments(self, x402_client: X402Client) -> ReqwestWithPaymentsBuilder<A>;
}

impl ReqwestWithPayments<reqwest::Client> for reqwest::Client {
    fn with_payments(self, x402_client: X402Client) -> ReqwestWithPaymentsBuilder<reqwest::Client> {
        ReqwestWithPaymentsBuilder {
            inner: self,
            x402_client,
        }
    }
}

pub struct ReqwestWithPaymentsBuilder<A> {
    inner: A,
    x402_client: X402Client,
}

pub trait ReqwestWithPaymentsBuild {
    type BuildResult;
    type BuilderResult;

    /// Finalize the middleware-enhanced client, producing a [`ClientWithMiddleware`].
    fn build(self) -> Self::BuildResult;

    /// Produce a [`Self::BuildResult`] to further customize the reqwest http client.
    fn builder(self) -> Self::BuilderResult;
}

impl ReqwestWithPaymentsBuild for ReqwestWithPaymentsBuilder<Client> {
    type BuildResult = ClientWithMiddleware;
    type BuilderResult = rqm::ClientBuilder;

    fn build(self) -> Self::BuildResult {
        self.builder().build()
    }

    fn builder(self) -> Self::BuilderResult {
        rqm::ClientBuilder::new(self.inner).with(self.x402_client)
    }
}

impl ReqwestWithPaymentsBuild for ReqwestWithPaymentsBuilder<ClientBuilder> {
    type BuildResult = Result<ClientWithMiddleware, reqwest::Error>;
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

pub struct V2Eip155ExactClient<S> {
    signer: S
}

impl<S> V2Eip155ExactClient<S> {
    pub fn new(signer: S) -> Self {
        Self { signer }
    }
}

type Amount = ruint::Uint<256, 4>;

pub struct PaymentProposal {
    pub amount: Amount,
    pub asset: String,
    pub chain_id: ChainId,
}

pub trait X402SchemeClient {

}

impl<S> X402SchemeClient for V2Eip155ExactClient<S> {

}

#[async_trait::async_trait]
impl rqm::Middleware for X402Client {
    /// Intercepts the response. If it's a 402, it constructs a payment and retries the request.
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> rqm::Result<Response> {
        let retry_req = req.try_clone(); // For retrying with payment later

        let res = next.clone().run(req, extensions).await?;

        #[cfg(feature = "telemetry")]
        tracing::debug!("Received response: {}", res.status());

        if res.status() != StatusCode::PAYMENT_REQUIRED {
            return Ok(res); // No 402 needed: passthrough
        }

        #[cfg(feature = "telemetry")]
        tracing::debug!("Received 402 Payment Required");
        let bytes = res
            .headers()
            .get("Payment-Required")
            .and_then(|h| Base64Bytes::from(h.as_bytes()).decode().ok());
        let k = bytes.and_then(|k| serde_json::from_slice::<serde_json::Value>(&k).ok());
        println!("decoded {:?}", k);
        if let Some(k) = k {
            let version = k.get("x402Version").and_then(|v| serde_json::from_value::<proto::X402Version>(v.clone()).ok());
            let accepts = k.get("accepts");
            println!("version {:?}", version);
            println!("accepts {:?}", accepts);
        }

        //     let payment_required_response = res.json::<PaymentRequiredResponse>().await?;
        //
        //     let retry_req = async {
        //         let payment_header = self
        //             .build_payment_header(&payment_required_response.accepts)
        //             .await?;
        //         let mut req = retry_req.ok_or(X402PaymentsError::RequestNotCloneable)?;
        //         let headers = req.headers_mut();
        //         headers.insert("X-Payment", payment_header);
        //         headers.insert(
        //             "Access-Control-Expose-Headers",
        //             HeaderValue::from_static("X-Payment-Response"),
        //         );
        //         Ok::<Request, X402PaymentsError>(req)
        //     }
        //     .await
        //     .map_err(Into::<rqm::Error>::into)?;
        next.run(retry_req.unwrap(), extensions).await
    }
}
