//! Experimental x402 client implementation
//!
//! This module contains the experimental implementation of the x402 client
//! with support for both V1 and V2 protocols, and a flexible scheme-based architecture.

use alloy_primitives::{Bytes, FixedBytes, U256};
use alloy_signer::Signer;
use alloy_sol_types::{SolStruct, eip712_domain, sol};
use http::Extensions;
use rand::{Rng, rng};
use reqwest::{Client, ClientBuilder, Request, Response, StatusCode};
use reqwest_middleware as rqm;
use reqwest_middleware::{ClientWithMiddleware, Next};
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use std::sync::Arc;
use x402_rs::chain::{ChainId, ChainIdPattern};
use x402_rs::chain::eip155::{ChecksummedAddress, Eip155ChainReference};
use x402_rs::proto::util::TokenAmount;
use x402_rs::proto::v2;
use x402_rs::scheme::v1_eip155_exact::{ExactEvmPayload, ExactEvmPayloadAuthorization, PaymentRequirementsExtra};
use x402_rs::scheme::v2_eip155_exact::types as v2_eip155_types;
use x402_rs::scheme::X402SchemeId;
use x402_rs::timestamp::UnixTimestamp;
use x402_rs::util::b64::Base64Bytes;

// EIP-712 struct for TransferWithAuthorization (ERC-3009)
sol! {
    #[derive(Serialize, Deserialize)]
    struct TransferWithAuthorization {
        address from;
        address to;
        uint256 value;
        uint256 validAfter;
        uint256 validBefore;
        bytes32 nonce;
    }
}

// ============================================================================
// Local PaymentRequirements with decimal amount serialization
// ============================================================================

/// Local version of PaymentRequirements that serializes amount as decimal string
/// and addresses as EIP-55 checksummed strings
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPaymentRequirements {
    pub scheme: String,
    pub network: ChainId,
    pub amount: TokenAmount,
    pub pay_to: ChecksummedAddress,
    pub max_timeout_seconds: u64,
    pub asset: ChecksummedAddress,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<PaymentRequirementsExtra>,
}

impl From<v2_eip155_types::PaymentRequirements> for LocalPaymentRequirements {
    fn from(req: v2_eip155_types::PaymentRequirements) -> Self {
        Self {
            scheme: req.scheme.to_string(),
            network: req.network,
            amount: req.amount.into(),
            pay_to: req.pay_to.into(),
            max_timeout_seconds: req.max_timeout_seconds,
            asset: req.asset.into(),
            extra: req.extra.map(|e| PaymentRequirementsExtra {
                name: e.name,
                version: e.version,
            }),
        }
    }
}

/// Local version of PaymentPayload that uses LocalPaymentRequirements
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPaymentPayload {
    pub x402_version: v2::X402Version2,
    pub accepted: LocalPaymentRequirements,
    pub resource: v2::ResourceInfo,
    pub payload: ExactEvmPayload,
}

// ============================================================================
// Error Types
// ============================================================================

#[derive(Debug, thiserror::Error)]
pub enum X402Error {
    #[error("No matching payment option found")]
    NoMatchingPaymentOption,

    #[error("Request is not cloneable (streaming body?)")]
    RequestNotCloneable,

    #[error("Failed to parse 402 response: {0}")]
    ParseError(String),

    #[error("Failed to sign payment: {0}")]
    SigningError(String),

    #[error("JSON error: {0}")]
    JsonError(#[from] serde_json::Error),
}

impl From<X402Error> for rqm::Error {
    fn from(error: X402Error) -> Self {
        rqm::Error::Middleware(error.into())
    }
}

/// Structured representation of a V2 Payment-Required header.
/// This provides proper typing for the payment required response.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PaymentRequiredV2 {
    pub x402_version: v2::X402Version2,
    pub resource: Option<v2::ResourceInfo>,
    pub accepts: Vec<serde_json::Value>,
}

// ============================================================================
// PaymentCandidate - Common intermediate type for selection
// ============================================================================

/// Represents a parsed payment option that can be compared across different schemes.
/// This is the common type used for selection before signing.
#[derive(Clone)]
pub struct PaymentCandidate {
    /// The chain ID (e.g., "eip155:84532" for Base Sepolia)
    pub chain_id: ChainId,
    /// Normalized asset address as string
    pub asset: String,
    /// Payment amount
    pub amount: U256,
    /// Scheme name (e.g., "exact")
    pub scheme: String,
    /// Protocol version (1 or 2)
    pub x402_version: u8,
    /// Reference to the scheme client that can handle this
    pub(crate) client: Arc<dyn X402SchemeClient>,
    /// Raw proposal data for re-parsing during signing
    pub(crate) raw_proposal: serde_json::Value,
    /// Resource info (V2 only)
    pub(crate) resource: Option<v2::ResourceInfo>,
}

// ============================================================================
// PaymentSelector - Selection strategy
// ============================================================================

/// Trait for selecting the best payment candidate from available options.
pub trait PaymentSelector: Send + Sync {
    fn select<'a>(&self, candidates: &'a [PaymentCandidate]) -> Option<&'a PaymentCandidate>;
}

/// Default selector: returns the first matching candidate.
/// Order is determined by registration order of scheme clients.
pub struct FirstMatch;

impl PaymentSelector for FirstMatch {
    fn select<'a>(&self, candidates: &'a [PaymentCandidate]) -> Option<&'a PaymentCandidate> {
        candidates.first()
    }
}

/// Selector that prefers a specific chain, falling back to first match.
#[allow(dead_code)]
pub struct PreferChain(pub ChainId);

impl PaymentSelector for PreferChain {
    fn select<'a>(&self, candidates: &'a [PaymentCandidate]) -> Option<&'a PaymentCandidate> {
        candidates
            .iter()
            .find(|c| c.chain_id == self.0)
            .or_else(|| candidates.first())
    }
}

/// Selector that only accepts payments up to a maximum amount.
#[allow(dead_code)]
pub struct MaxAmount(pub U256);

impl PaymentSelector for MaxAmount {
    fn select<'a>(&self, candidates: &'a [PaymentCandidate]) -> Option<&'a PaymentCandidate> {
        candidates.iter().find(|c| c.amount <= self.0)
    }
}

// ============================================================================
// X402SchemeClient - Trait for scheme-specific handling
// ============================================================================

/// Trait implemented by scheme-specific clients (e.g., V2Eip155ExactClient).
/// Each implementation handles a specific combination of protocol version,
/// chain namespace, and payment scheme.
///
/// Extends `X402SchemeId` which provides:
/// - `x402_version()` - protocol version (1 or 2)
/// - `namespace()` - chain namespace ("eip155", "solana")
/// - `scheme()` - payment scheme name ("exact")
#[async_trait::async_trait]
pub trait X402SchemeClient: X402SchemeId + Send + Sync {
    /// Parse the raw accepts entry and extract common fields for selection.
    fn to_candidate(
        &self,
        raw: &serde_json::Value,
        client: Arc<dyn X402SchemeClient>,
        resource: Option<v2::ResourceInfo>,
    ) -> Result<PaymentCandidate, X402Error>;

    /// Sign the payment for the selected candidate.
    /// Returns the value for the X-Payment header (base64 encoded).
    async fn sign_payment(&self, candidate: &PaymentCandidate) -> Result<String, X402Error>;
}

// ============================================================================
// RegisteredSchemeClient - Internal wrapper for pattern-based matching
// ============================================================================

/// Internal wrapper that pairs a scheme client with its chain pattern.
struct RegisteredSchemeClient {
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
    fn matches(&self, version: u8, scheme: &str, chain_id: &ChainId) -> bool {
        self.client.x402_version() == version
            && self.client.scheme() == scheme
            && self.client.namespace() == chain_id.namespace()
            && self.pattern.matches(chain_id)
    }
}

// ============================================================================
// V2Eip155ExactClient - Implementation for V2 EVM exact payments
// ============================================================================

/// Client for handling V2 protocol, EIP-155 chains, "exact" scheme payments.
pub struct V2Eip155ExactClient<S> {
    signer: Arc<S>,
}

impl<S> V2Eip155ExactClient<S> {
    pub fn new(signer: S) -> Self {
        Self {
            signer: Arc::new(signer),
        }
    }
}

impl<S> X402SchemeId for V2Eip155ExactClient<S> {
    fn x402_version(&self) -> u8 {
        2
    }

    fn namespace(&self) -> &str {
        "eip155"
    }

    fn scheme(&self) -> &str {
        "exact"
    }
}

#[async_trait::async_trait]
impl<S: Signer + Send + Sync> X402SchemeClient for V2Eip155ExactClient<S> {
    fn to_candidate(
        &self,
        raw: &serde_json::Value,
        client: Arc<dyn X402SchemeClient>,
        resource: Option<v2::ResourceInfo>,
    ) -> Result<PaymentCandidate, X402Error> {
        // Parse into scheme-specific type
        let req: v2_eip155_types::PaymentRequirements = serde_json::from_value(raw.clone())?;

        Ok(PaymentCandidate {
            chain_id: req.network.clone(),
            asset: req.asset.to_string(),
            amount: req.amount,
            scheme: "exact".into(),
            x402_version: 2,
            client,
            raw_proposal: raw.clone(),
            resource,
        })
    }

    async fn sign_payment(&self, candidate: &PaymentCandidate) -> Result<String, X402Error> {
        // Re-parse to get full typed requirements
        let req: v2_eip155_types::PaymentRequirements =
            serde_json::from_value(candidate.raw_proposal.clone())?;

        // Get token name/version from extra
        let (name, version) = match &req.extra {
            None => ("".to_string(), "".to_string()),
            Some(extra) => (extra.name.clone(), extra.version.clone()),
        };

        // Get chain ID for EIP-712 domain using proper Eip155ChainReference
        let chain_reference = Eip155ChainReference::try_from(candidate.chain_id.clone())
            .map_err(|e| X402Error::SigningError(format!("Invalid EIP-155 chain reference: {e}")))?;
        let chain_id_num = chain_reference.inner();

        // Build EIP-712 domain
        let domain = eip712_domain! {
            name: name,
            version: version,
            chain_id: chain_id_num,
            verifying_contract: req.asset,
        };

        // Build authorization
        let now = UnixTimestamp::now();
        // valid_after should be in the past (10 minutes ago) to ensure the payment is immediately valid
        let valid_after_secs = now.as_secs().saturating_sub(10 * 60);
        let valid_after = UnixTimestamp::from_secs(valid_after_secs);
        let valid_before = now + req.max_timeout_seconds;
        let nonce: [u8; 32] = rng().random();

        let authorization = ExactEvmPayloadAuthorization {
            from: self.signer.address().into(),
            to: req.pay_to.into(),
            value: req.amount.into(),
            valid_after,
            valid_before,
            nonce: FixedBytes(nonce),
        };

        // Create the EIP-712 struct for signing
        // IMPORTANT: The values here MUST match the authorization struct exactly,
        // as the facilitator will reconstruct this struct from the authorization
        // to verify the signature.
        let transfer_with_authorization = TransferWithAuthorization {
            from: authorization.from.into(),
            to: authorization.to.into(),
            value: authorization.value.into(),
            validAfter: U256::from(authorization.valid_after.as_secs()),
            validBefore: U256::from(authorization.valid_before.as_secs()),
            nonce: FixedBytes(nonce),
        };

        let eip712_hash = transfer_with_authorization.eip712_signing_hash(&domain);
        let signature = self
            .signer
            .sign_hash(&eip712_hash)
            .await
            .map_err(|e| X402Error::SigningError(format!("{e:?}")))?;

        // Build the payment payload
        let resource = candidate
            .resource
            .clone()
            .ok_or_else(|| X402Error::SigningError("Missing resource info".into()))?;

        let payload = LocalPaymentPayload {
            x402_version: v2::X402Version2,
            accepted: req.into(),
            resource,
            payload: ExactEvmPayload {
                signature: signature.as_bytes().to_vec().into(),
                authorization,
            },
        };

        // Encode as base64 for header
        let json = serde_json::to_vec(&payload)?;
        let b64 = Base64Bytes::encode(json);
        // Convert to string via UTF-8 since base64 is ASCII
        let header_value = String::from_utf8(b64.as_ref().to_vec())
            .map_err(|e| X402Error::SigningError(format!("Base64 encoding error: {e}")))?;
        Ok(header_value)
    }
}

// ============================================================================
// X402Client - Main client that holds scheme clients and selector
// ============================================================================

/// The main x402 client that orchestrates scheme clients and selection.
pub struct X402Client {
    /// Registered scheme clients with their chain patterns
    schemes: Vec<RegisteredSchemeClient>,
    selector: Arc<dyn PaymentSelector>,
}

impl X402Client {
    pub fn new() -> Self {
        Self {
            schemes: vec![],
            selector: Arc::new(FirstMatch),
        }
    }

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
    pub fn with_selector<P: PaymentSelector + 'static>(mut self, selector: P) -> Self {
        self.selector = Arc::new(selector);
        self
    }

    /// Parse a 402 response and build candidates from all registered scheme clients.
    fn build_candidates(
        &self,
        response: &Response,
    ) -> Result<(Vec<PaymentCandidate>, u8), X402Error> {
        // Try V2 first (header-based)
        if let Some(header) = response.headers().get("Payment-Required") {
            let bytes = Base64Bytes::from(header.as_bytes())
                .decode()
                .map_err(|e| X402Error::ParseError(format!("Base64 decode failed: {e}")))?;

            // Parse directly into typed PaymentRequiredV2 struct
            let payment_required: PaymentRequiredV2 = serde_json::from_slice(&bytes)
                .map_err(|e| X402Error::ParseError(format!("Failed to parse PaymentRequiredV2: {e}")))?;

            // Version is already validated by the X402Version2 deserializer
            return self.build_candidates_from_accepts(
                &payment_required.accepts,
                payment_required.x402_version.into(),
                payment_required.resource,
            );
        }

        // TODO: V1 fallback (body-based) - would need to consume response body
        // For now, return error
        Err(X402Error::ParseError(
            "V1 protocol not yet implemented".into(),
        ))
    }

    fn build_candidates_from_accepts(
        &self,
        accepts: &[serde_json::Value],
        version: u8,
        resource: Option<v2::ResourceInfo>,
    ) -> Result<(Vec<PaymentCandidate>, u8), X402Error> {
        let mut candidates = Vec::new();

        for raw in accepts {
            let scheme = raw.get("scheme").and_then(|v| v.as_str()).unwrap_or("");
            let network = raw.get("network").and_then(|v| v.as_str()).unwrap_or("");

            // Parse network string into ChainId
            let chain_id = match ChainId::from_str(network) {
                Ok(id) => id,
                Err(_) => continue, // Skip invalid network formats
            };

            // Find matching registered client
            for registered in self.schemes.iter() {
                if registered.matches(version, scheme, &chain_id) {
                    if let Ok(candidate) =
                        registered
                            .client
                            .to_candidate(raw, registered.client.clone(), resource.clone())
                    {
                        candidates.push(candidate);
                        break; // First matching client wins for this entry
                    }
                }
            }
        }

        Ok((candidates, version))
    }
}

impl Default for X402Client {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Middleware implementation
// ============================================================================

#[async_trait::async_trait]
impl rqm::Middleware for X402Client {
    async fn handle(
        &self,
        req: Request,
        extensions: &mut Extensions,
        next: Next<'_>,
    ) -> rqm::Result<Response> {
        let retry_req = req.try_clone();
        let res = next.clone().run(req, extensions).await?;

        if res.status() != StatusCode::PAYMENT_REQUIRED {
            return Ok(res);
        }

        println!("Received 402 Payment Required");

        // Build candidates from the 402 response
        let (candidates, _version) = self
            .build_candidates(&res)
            .map_err(Into::<rqm::Error>::into)?;

        println!("Found {} candidates", candidates.len());
        for (i, c) in candidates.iter().enumerate() {
            println!(
                "  [{}] chain={}, asset={}, amount={}",
                i, c.chain_id, c.asset, c.amount
            );
        }

        // Select the best candidate
        let selected = self
            .selector
            .select(&candidates)
            .ok_or(X402Error::NoMatchingPaymentOption)?;

        println!(
            "Selected candidate: chain={}, amount={}",
            selected.chain_id, selected.amount
        );

        // Sign the payment using the client reference stored in the candidate
        let payment_header = selected
            .client
            .sign_payment(selected)
            .await
            .map_err(Into::<rqm::Error>::into)?;

        println!("Payment header length: {} bytes", payment_header.len());

        // Retry with payment
        let mut retry = retry_req.ok_or(X402Error::RequestNotCloneable)?;
        retry.headers_mut().insert(
            "PAYMENT-SIGNATURE",
            payment_header
                .parse()
                .map_err(|e| X402Error::SigningError(format!("{e}")))?,
        );

        next.run(retry, extensions).await
    }
}

// ============================================================================
// Builder traits for ergonomic API
// ============================================================================

pub trait ReqwestWithPayments<A> {
    fn with_payments(self, x402_client: X402Client) -> ReqwestWithPaymentsBuilder<A>;
}

impl ReqwestWithPayments<Client> for Client {
    fn with_payments(self, x402_client: X402Client) -> ReqwestWithPaymentsBuilder<Client> {
        ReqwestWithPaymentsBuilder {
            inner: self,
            x402_client,
        }
    }
}

impl ReqwestWithPayments<ClientBuilder> for ClientBuilder {
    fn with_payments(self, x402_client: X402Client) -> ReqwestWithPaymentsBuilder<ClientBuilder> {
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

    fn build(self) -> Self::BuildResult;
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
