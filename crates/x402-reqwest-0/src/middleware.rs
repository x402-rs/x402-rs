//! Middleware for handling HTTP 402 Payment Required responses using the x402 protocol.
//!
//! This module provides the `X402Payments` struct which implements `reqwest_middleware::Middleware`,
//! allowing automatic retries of requests with valid `X-Payment` headers constructed via a signer.
//!
//! It includes:
//! - Selection of preferred payment methods
//! - Max token enforcement
//! - EIP-712-based payload construction and signing
//! - Base64 encoding into a payment header

use http::{Extensions, HeaderValue, StatusCode};
use reqwest::{Request, Response};
use reqwest_middleware as rqm;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::SystemTimeError;
use tracing::instrument;
use x402_rs::network::{Network, USDCDeployment};
use x402_rs::types::{
    Base64Bytes, MixedAddressError, MoneyAmount, MoneyAmountParseError, PaymentPayload,
    PaymentRequiredResponse, PaymentRequirements, TokenAmount, TokenAsset, TokenDeployment,
};

use crate::chains::{IntoSenderWallet, SenderWallet};

/// Represents the maximum allowed amount for a specific token asset.
pub struct MaxTokenAmount {
    asset: TokenAsset,
    amount: TokenAmount,
}

/// Trait for converting from a token amount directly into a MaxTokenAmount bound.
pub trait MaxTokenAmountFromTokenAmount {
    fn token_amount<A: Into<TokenAmount>>(&self, token_amount: A) -> MaxTokenAmount;
}

/// Trait for converting from a user-friendly amount (e.g., "1.0 USDC")
/// into a token-denominated max bound, respecting decimals.
pub trait MaxTokenAmountFromAmount {
    type Error;
    fn amount<A: TryInto<MoneyAmount>>(&self, amount: A) -> Result<MaxTokenAmount, Self::Error>;
}

impl MaxTokenAmountFromTokenAmount for TokenAsset {
    fn token_amount<A: Into<TokenAmount>>(&self, token_amount: A) -> MaxTokenAmount {
        MaxTokenAmount {
            asset: self.clone(),
            amount: token_amount.into(),
        }
    }
}

impl MaxTokenAmountFromTokenAmount for TokenDeployment {
    fn token_amount<A: Into<TokenAmount>>(&self, token_amount: A) -> MaxTokenAmount {
        MaxTokenAmount {
            asset: self.asset.clone(),
            amount: token_amount.into(),
        }
    }
}

/// Errors that can occur while constructing or applying an x402 payment.
#[derive(Debug, thiserror::Error)]
pub enum X402PaymentsError {
    /// Occurs when a value fails to convert into a [`MoneyAmount`],
    /// for example, parsing a string like `"1.0"` fails due to formatting or type mismatch.
    #[error("Failed to convert to MoneyAmount")]
    MoneyAmountConversion,
    /// Occurs when a [`MoneyAmount`] cannot be converted into a [`TokenAmount`],
    /// typically due to a decimal mismatch or overflow,
    /// for example, trying to convert `0.00000000001` to a USDC token amount (which has 6 decimals).
    #[error("Failed to convert to TokenAmount")]
    TokenAmountConversion(#[source] MoneyAmountParseError),
    /// Triggered when the selected payment amount exceeds the configured maximum for that token.
    /// This prevents accidental or malicious overspending.
    #[error("Payment amount {requested} exceeds maximum allowed {allowed} for token {asset}")]
    PaymentAmountTooLarge {
        requested: TokenAmount,
        allowed: TokenAmount,
        asset: TokenAsset,
    },
    /// Indicates that the original request could not be cloned for retrying with a payment header.
    /// This typically happens when the request body is a stream or otherwise non-reusable.
    #[error("Request object is not cloneable. Are you passing a streaming body?")]
    RequestNotCloneable,
    /// Raised when none of the server's accepted payment methods match the client's preferred tokens.
    /// Includes both the accepted and preferred sets to aid debugging.
    #[error("No matching payment method found. Accepted: {accepts:?}. Preferred: {prefer:?}")]
    NoSuitablePaymentMethod {
        accepts: Vec<PaymentRequirements>,
        prefer: Vec<TokenAsset>,
    },
    /// Raised when an EVM address (e.g., `to`, `from`, or `verifying_contract`) is invalid or cannot be parsed.
    #[error("Invalid EVM address")]
    InvalidEVMAddress(#[source] MixedAddressError),
    /// Raised when the system clock could not be read to compute `validAfter`/`validBefore` timestamps.
    /// Should be an extremely rare occurrence.
    #[error("Failed to get system clock")]
    ClockError(#[source] SystemTimeError),
    /// Indicates that signing the EIP-712 payment payload failed using the provided signer.
    #[error("Failed to sign payment payload: {0}")]
    SigningError(String),
    /// Occurs if the constructed payment payload cannot be serialized to JSON.
    /// This should be an extremely rare occurrence.
    #[error("Failed to encode payment payload to json")]
    JsonEncodeError(#[source] serde_json::Error),
    /// Raised when the base64-encoded JSON payload cannot be inserted into a [`HeaderValue`].
    /// Typically caused by invalid characters or excessive length.
    #[error("Failed to encode payment payload to HTTP header")]
    HeaderValueEncodeError(#[source] http::header::InvalidHeaderValue),
}

impl From<X402PaymentsError> for rqm::Error {
    fn from(error: X402PaymentsError) -> Self {
        rqm::Error::Middleware(error.into())
    }
}

impl MaxTokenAmountFromAmount for TokenDeployment {
    type Error = X402PaymentsError;
    fn amount<A: TryInto<MoneyAmount>>(&self, amount: A) -> Result<MaxTokenAmount, Self::Error> {
        let money_amount = amount
            .try_into()
            .map_err(|_| Self::Error::MoneyAmountConversion)?;
        let decimals = self.decimals;
        let token_amount = money_amount
            .as_token_amount(decimals as u32)
            .map_err(Self::Error::TokenAmountConversion)?;
        Ok(MaxTokenAmount {
            asset: self.asset.clone(),
            amount: token_amount,
        })
    }
}

/// Middleware that handles automatic retries for HTTP 402 responses
/// by attaching a valid x402 payment header.
#[derive(Clone)]
pub struct X402Payments {
    wallets: Vec<Arc<dyn SenderWallet>>,
    max_token_amount: HashMap<TokenAsset, TokenAmount>,
    prefer: Vec<TokenAsset>,
}

impl X402Payments {
    pub fn with_wallet<S: IntoSenderWallet>(wallet: S) -> Self {
        Self {
            wallets: vec![wallet.into_sender_wallet()],
            max_token_amount: HashMap::new(),
            prefer: vec![],
        }
    }

    pub fn and_with_wallet<S: IntoSenderWallet>(self, wallet: S) -> Self {
        let mut wallets = self.wallets;
        wallets.push(wallet.into_sender_wallet());
        Self {
            wallets,
            max_token_amount: self.max_token_amount,
            prefer: self.prefer,
        }
    }

    /// Set a max amount allowed for a given token.
    pub fn max(&self, max: MaxTokenAmount) -> Self {
        let mut this = self.clone();
        this.max_token_amount.insert(max.asset, max.amount);
        this
    }

    /// Extend the preferred token list, prioritizing what the client wants to pay with.
    pub fn prefer<T: Into<Vec<TokenAsset>>>(&self, prefer: T) -> Self {
        let mut this = self.clone();
        this.prefer.append(&mut prefer.into());
        this
    }

    /// Selects the most preferred payment requirement based on the client's `prefer` list
    /// and network priority (Base preferred).
    pub fn select_payment_requirements(
        &self,
        payment_requirements: &[PaymentRequirements],
    ) -> Result<PaymentRequirements, X402PaymentsError> {
        let mut sorted: Vec<PaymentRequirements> = payment_requirements.to_vec();
        // Assign priority score: lower is better
        // Prefer what is in self.prefer and ultimately Base
        sorted.sort_by_key(|req| {
            let pref_index = self
                .prefer
                .iter()
                .position(|a| a == &req.token_asset())
                .unwrap_or(usize::MAX);
            let base_priority = if req.network == Network::Base { 0 } else { 1 };
            (pref_index, base_priority)
        });

        #[cfg(feature = "telemetry")]
        {
            for (i, req) in sorted.iter().enumerate() {
                tracing::debug!(index = i, asset = ?req.asset, network = ?req.network, "Ranked candidate payment requirement");
            }
        }

        // Try to find a USDC requirement
        let usdc_requirement = sorted.iter().find(|req| {
            let usdc = USDCDeployment::by_network(req.network);
            req.asset == usdc.address()
        });

        let selected = usdc_requirement
            .cloned() // Prioritize USDC requirements if available
            .or_else(|| sorted.into_iter().next()); // If no USDC requirements are found, return the first accepted requirement.

        selected.ok_or(X402PaymentsError::NoSuitablePaymentMethod {
            accepts: payment_requirements.to_vec(),
            prefer: self.prefer.clone(),
        })
    }

    /// Ensures that the selected requirement does not exceed the max configured amount.
    pub fn assert_max_amount(
        &self,
        selected: &PaymentRequirements,
    ) -> Result<(), X402PaymentsError> {
        let token_asset = selected.token_asset();
        if let Some(max) = self.max_token_amount.get(&token_asset)
            && &selected.max_amount_required > max
        {
            return Err(X402PaymentsError::PaymentAmountTooLarge {
                requested: selected.max_amount_required,
                allowed: *max,
                asset: token_asset,
            });
        }
        Ok(())
    }

    /// Constructs a [`PaymentPayload`] for a given requirement by generating
    /// a nonce and signing an EIP-712 [`TransferWithAuthorization`] struct.
    #[instrument(name = "x402.make_payment_payload", skip_all, fields(
        network = ?selected.network,
        token = ?selected.asset,
        amount = %selected.max_amount_required,
    ))]
    pub async fn make_payment_payload(
        &self,
        selected: PaymentRequirements,
    ) -> Result<PaymentPayload, X402PaymentsError> {
        let wallet = self.wallets.iter().find(|w| w.can_handle(&selected));
        match wallet {
            None => Err(X402PaymentsError::SigningError(
                "No suitable wallet found".to_string(),
            )),
            Some(wallet) => wallet.payment_payload(selected).await,
        }
    }

    /// Encodes the `PaymentPayload` into a base64 string suitable for an `X-Payment` header.
    pub fn encode_payment_header(
        payload: &PaymentPayload,
    ) -> Result<HeaderValue, X402PaymentsError> {
        let json = serde_json::to_vec(payload).map_err(X402PaymentsError::JsonEncodeError)?;
        let b64 = Base64Bytes::encode(json);
        HeaderValue::from_bytes(b64.as_ref()).map_err(X402PaymentsError::HeaderValueEncodeError)
    }

    /// Builds the payment header by selecting a requirement, enforcing max,
    /// constructing and signing the payload, and base64-encoding it.
    #[instrument(name = "x402.build_payment_header", skip(self))]
    pub async fn build_payment_header(
        &self,
        accepts: &[PaymentRequirements],
    ) -> Result<HeaderValue, X402PaymentsError> {
        let selected = self.select_payment_requirements(accepts)?;
        #[cfg(feature = "telemetry")]
        tracing::debug!(?selected, "Selected payment requirement");
        self.assert_max_amount(&selected)?;
        let payment_payload = self.make_payment_payload(selected).await?;
        Self::encode_payment_header(&payment_payload)
    }
}

#[async_trait::async_trait]
impl rqm::Middleware for X402Payments {
    /// Intercepts the response. If it's a 402, it constructs a payment and retries the request.
    #[instrument(name = "x402.handle", skip(self, req, extensions, next), fields(method = %req.method(), url = %req.url()))]
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: rqm::Next<'_>,
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

        let payment_required_response = res.json::<PaymentRequiredResponse>().await?;

        let retry_req = async {
            let payment_header = self
                .build_payment_header(&payment_required_response.accepts)
                .await?;
            let mut req = retry_req.ok_or(X402PaymentsError::RequestNotCloneable)?;
            let headers = req.headers_mut();
            headers.insert("X-Payment", payment_header);
            headers.insert(
                "Access-Control-Expose-Headers",
                HeaderValue::from_static("X-Payment-Response"),
            );
            Ok::<Request, X402PaymentsError>(req)
        }
        .await
        .map_err(Into::<rqm::Error>::into)?;
        next.run(retry_req, extensions).await
    }
}
