use reqwest::{Client, ClientBuilder};
use reqwest_middleware as rqm;

use crate::client::X402Client;

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
