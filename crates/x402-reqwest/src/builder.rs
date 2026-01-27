//! Builder utilities for integrating x402 with reqwest.
//!
//! This module provides traits and types for building reqwest clients
//! with x402 payment middleware.

use reqwest::{Client, ClientBuilder};
use reqwest_middleware as rqm;

use crate::client::X402Client;

/// Trait for adding x402 payment handling to reqwest clients.
///
/// This trait is implemented on [`Client`] and [`ClientBuilder`], allowing
/// you to create a reqwest client with automatic x402 payment handling.
///
/// ## Example
///
/// ```rust,no_run
/// use x402_reqwest::{ReqwestWithPayments, ReqwestWithPaymentsBuild, X402Client};
/// use x402_chain_eip155::V1Eip155ExactClient;
/// use alloy_signer_local::PrivateKeySigner;
/// use std::sync::Arc;
/// use reqwest::Client;
///
/// let signer = Arc::new("PRIVATE_KEY".parse::<PrivateKeySigner>().unwrap());
/// let x402_client = X402Client::new()
///     .register(V1Eip155ExactClient::new(signer));
///
/// let http_client = Client::new()
///     .with_payments(x402_client)
///     .build();
/// ```
pub trait ReqwestWithPayments<A, S> {
    /// Adds x402 payment middleware to the client or builder.
    ///
    /// # Arguments
    ///
    /// * `x402_client` - The x402 client configured with scheme handlers
    ///
    /// # Returns
    ///
    /// A builder that can be used to build the final client.
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

/// Builder for creating a reqwest client with x402 middleware.
pub struct ReqwestWithPaymentsBuilder<A, S> {
    inner: A,
    x402_client: X402Client<S>,
}

/// Trait for building the final client from a [`ReqwestWithPaymentsBuilder`].
pub trait ReqwestWithPaymentsBuild {
    /// The type returned by [`build`]
    type BuildResult;
    /// The type returned by [`builder`]
    type BuilderResult;

    /// Builds the client, consuming the builder.
    fn build(self) -> Self::BuildResult;

    /// Returns the underlying reqwest client builder with middleware added.
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
