//! Facilitator-side payment verification and settlement for V2 EIP-155 exact scheme.
//!
//! This module implements the facilitator logic for V2 protocol payments on EVM chains.
//! It reuses most of the V1 verification and settlement logic but handles V2-specific
//! payload structures with embedded requirements and CAIP-2 chain IDs.

pub mod eip2612;
pub mod eip3009;
pub mod permit2;

use crate::attribution_tag::{AttributionTag, AttributionTagExtension, is_valid_code};
use alloy_provider::Provider;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use x402_types::chain::ChainProviderOps;
use x402_types::proto;
use x402_types::proto::v2;
use x402_types::scheme::{
    ExtensionKey, X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError,
};
#[cfg(feature = "telemetry")]
use x402_types::util::telemetry::record_payment_context;

use crate::V2Eip155Exact;
use crate::chain::Eip155MetaTransactionProvider;
use crate::eip2612_gas_sponsoring::Eip2612GasSponsoring;
use crate::v1_eip155_exact::ExactScheme;
use crate::v1_eip155_exact::facilitator::Eip155ExactError;
use crate::v2_eip155_exact::types;

impl<P> X402SchemeFacilitatorBuilder<P> for V2Eip155Exact
where
    P: Eip155MetaTransactionProvider + ChainProviderOps + Send + Sync + 'static,
    Eip155ExactError: From<P::Error>,
{
    fn build(
        &self,
        provider: P,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let config = V2Eip155ExactFacilitatorConfig::from_json(config)?;
        Ok(Box::new(V2Eip155ExactFacilitator::new(provider, config)))
    }
}

/// Configuration for the V2 EIP-155 exact scheme facilitator.
///
/// This struct holds optional configuration parameters that control
/// the facilitator's behavior for V2 exact payments on EVM chains.
///
/// # Fields
///
/// - `eip2612_gas_sponsoring`: Whether to enable EIP-2612 gas-sponsoring extension.
///   When enabled, the facilitator supports atomic settlement with EIP-2612 permits,
///   allowing the payer to have their gas fees covered by the facilitator.
/// - `attribution_tag`: The facilitator's own ERC-8021 attribution code, written as `w`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct V2Eip155ExactFacilitatorConfig {
    #[serde(default)]
    pub eip2612_gas_sponsoring: bool,
    /// ERC-8021 attribution: the facilitator's own code, written as `w` (wallet).
    /// When set, the facilitator appends an attribution suffix to EIP-3009
    /// settlement calldata, combining this code with any `a`/`s` codes the
    /// payment carries in its `builder-code` extension. Must match
    /// `^[a-z0-9_]{1,32}$`; an invalid value fails [`X402SchemeFacilitatorBuilder::build`].
    /// Unset (the default) leaves `w` out.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attribution_tag: Option<String>,
}

impl V2Eip155ExactFacilitatorConfig {
    /// Reads the scheme config, falling back to the defaults when it is absent
    /// or does not deserialize. A present but malformed `attribution_tag` is an
    /// error: a facilitator asked to tag its settlements must not run untagged.
    pub fn from_json(
        config: Option<serde_json::Value>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let config: Self = config
            .and_then(|config| Self::deserialize(config).ok())
            .unwrap_or_default();
        if let Some(tag) = config.attribution_tag.as_deref()
            && !is_valid_code(tag)
        {
            return Err(
                format!("invalid attribution_tag {tag:?}: must match ^[a-z0-9_]{{1,32}}$").into(),
            );
        }
        Ok(config)
    }
}

/// Extra data for the V2 EIP-155 exact scheme facilitator.
///
/// This struct holds additional response data returned by the facilitator's
/// `supported` method, including supported extensions.
///
/// # Fields
///
/// - `extensions`: Optional list of supported extension identifiers.
///   These extensions indicate additional features the facilitator supports,
///   such as EIP-2612 gas sponsoring.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct V2Eip155ExactFacilitatorExtra {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extensions: Vec<String>,
}

/// Facilitator for V2 EIP-155 exact scheme payments.
///
/// This struct implements the [`X402SchemeFacilitator`] trait to provide payment
/// verification and settlement services for ERC-3009 based payments on EVM chains
/// using the V2 protocol.
///
/// # Type Parameters
///
/// - `P`: The provider type, which must implement [`Eip155MetaTransactionProvider`]
///   and [`ChainProviderOps`]
pub struct V2Eip155ExactFacilitator<P> {
    provider: P,
    eip2612_gas_sponsoring: bool,
    attribution_tag: Option<String>,
}

impl<P> V2Eip155ExactFacilitator<P> {
    /// Creates a new V2 EIP-155 exact scheme facilitator with the given provider.
    pub fn new(provider: P, config: V2Eip155ExactFacilitatorConfig) -> Self {
        Self {
            provider,
            eip2612_gas_sponsoring: config.eip2612_gas_sponsoring,
            attribution_tag: config.attribution_tag,
        }
    }
}

#[async_trait::async_trait]
impl<P> X402SchemeFacilitator for V2Eip155ExactFacilitator<P>
where
    P: Eip155MetaTransactionProvider + ChainProviderOps + Send + Sync,
    P::Inner: Provider,
    Eip155ExactError: From<P::Error>,
{
    #[cfg_attr(feature = "telemetry", tracing::instrument(skip_all, err, fields(
        otel.kind = "internal",
        chain_id = tracing::field::Empty,
        payer = tracing::field::Empty,
        pay_to = tracing::field::Empty
    )))]
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError> {
        let verify_request = types::FacilitatorVerifyRequest::try_from(request.clone())?;
        let verify_response = match verify_request {
            types::FacilitatorVerifyRequest::Eip3009 {
                payment_payload,
                payment_requirements,
                x402_version: _,
            } => {
                #[cfg(feature = "telemetry")]
                record_payment_context(
                    &payment_payload.accepted.network,
                    payment_payload.payload.authorization.from,
                    payment_requirements.pay_to,
                );
                eip3009::verify_eip3009_payment(
                    &self.provider,
                    &payment_payload,
                    &payment_requirements,
                )
                .await?
            }
            types::FacilitatorVerifyRequest::Permit2 {
                payment_payload,
                payment_requirements,
                x402_version: _,
            } => {
                #[cfg(feature = "telemetry")]
                {
                    let authorization = &payment_payload.payload.permit_2_authorization;
                    record_payment_context(
                        &payment_payload.accepted.network,
                        authorization.from,
                        payment_requirements.pay_to,
                    );
                }
                permit2::verify_permit2_payment(
                    &self.provider,
                    self.eip2612_gas_sponsoring,
                    &payment_payload,
                    &payment_requirements,
                )
                .await?
            }
        };
        Ok(verify_response.into())
    }

    #[cfg_attr(feature = "telemetry", tracing::instrument(skip_all, err, fields(
        otel.kind = "internal",
        chain_id = tracing::field::Empty,
        payer = tracing::field::Empty,
        pay_to = tracing::field::Empty
    )))]
    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, X402SchemeFacilitatorError> {
        let settle_request = types::FacilitatorSettleRequest::try_from(request.clone())?;
        let settle_response = match settle_request {
            types::FacilitatorSettleRequest::Eip3009 {
                payment_payload,
                payment_requirements,
                x402_version: _,
            } => {
                #[cfg(feature = "telemetry")]
                record_payment_context(
                    &payment_payload.accepted.network,
                    payment_payload.payload.authorization.from,
                    payment_requirements.pay_to,
                );
                // ERC-8021 attribution: combine the facilitator's configured
                // tag with the app/service codes the payment carries in its
                // `builder-code` extension, and append the resulting suffix to
                // the settlement calldata.
                let attribution_tag = AttributionTag::from_config_and_extensions(
                    self.attribution_tag.as_deref(),
                    &payment_payload.extensions,
                );
                let attribution_suffix = attribution_tag.suffix();
                eip3009::settle_eip3009_payment(
                    &self.provider,
                    &payment_payload,
                    &payment_requirements,
                    attribution_suffix.as_ref(),
                )
                .await?
            }
            types::FacilitatorSettleRequest::Permit2 {
                payment_requirements,
                payment_payload,
                x402_version: _,
            } => {
                #[cfg(feature = "telemetry")]
                {
                    let authorization = &payment_payload.payload.permit_2_authorization;
                    record_payment_context(
                        &payment_payload.accepted.network,
                        authorization.from,
                        payment_requirements.pay_to,
                    );
                }
                permit2::settle_permit2_payment(
                    &self.provider,
                    self.eip2612_gas_sponsoring,
                    &payment_payload,
                    &payment_requirements,
                )
                .await?
            }
        };
        Ok(settle_response.into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError> {
        let chain_id = self.provider.chain_id();
        let extensions =
            supported_extensions(self.eip2612_gas_sponsoring, self.attribution_tag.is_some());
        let extra = V2Eip155ExactFacilitatorExtra {
            extensions: extensions.clone(),
        };
        let extra = serde_json::to_value(extra).ok();
        let kinds = vec![proto::SupportedPaymentKind {
            x402_version: v2::X402Version2.into(),
            scheme: ExactScheme.to_string(),
            network: chain_id.clone().into(),
            extra,
        }];
        let signers = {
            let mut signers = HashMap::with_capacity(1);
            signers.insert(chain_id, self.provider.signer_addresses());
            signers
        };
        Ok(proto::SupportedResponse {
            kinds,
            extensions,
            signers,
        })
    }
}

/// The extension keys this facilitator advertises in `/supported`.
fn supported_extensions(eip2612_gas_sponsoring: bool, attribution_tag: bool) -> Vec<String> {
    let mut extensions = vec![];
    // Conditionally include EIP-2612 gas-sponsoring extension based on config.
    // This tells the client it may include an EIP-2612 permit in the payload,
    // allowing the facilitator to call `settleWithPermit` atomically.
    if eip2612_gas_sponsoring {
        extensions.push(Eip2612GasSponsoring::EXTENSION_KEY.to_string());
    }
    // Advertised only when the facilitator has its own code configured, so
    // `/supported` is unchanged for a facilitator that did not opt in.
    if attribution_tag {
        extensions.push(AttributionTagExtension::EXTENSION_KEY.to_string());
    }
    extensions
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn config_reads_attribution_tag() {
        let config = V2Eip155ExactFacilitatorConfig::from_json(Some(json!({
            "eip2612_gas_sponsoring": true,
            "attribution_tag": "celo_facil"
        })))
        .unwrap();
        assert!(config.eip2612_gas_sponsoring);
        assert_eq!(config.attribution_tag.as_deref(), Some("celo_facil"));
    }

    #[test]
    fn config_without_attribution_tag_is_unchanged() {
        let config = V2Eip155ExactFacilitatorConfig::from_json(Some(json!({
            "eip2612_gas_sponsoring": true
        })))
        .unwrap();
        assert!(config.eip2612_gas_sponsoring);
        assert_eq!(config.attribution_tag, None);
        let config = V2Eip155ExactFacilitatorConfig::from_json(None).unwrap();
        assert_eq!(config.attribution_tag, None);
    }

    #[test]
    fn config_rejects_a_malformed_attribution_tag() {
        for tag in ["Celo-Facil", "", "with space", &"a".repeat(33)] {
            let err = V2Eip155ExactFacilitatorConfig::from_json(Some(json!({
                "attribution_tag": tag
            })))
            .expect_err("malformed tag must be rejected");
            assert!(err.to_string().contains("invalid attribution_tag"), "{err}");
        }
    }

    #[test]
    fn supported_advertises_the_extension_only_with_a_tag_configured() {
        assert_eq!(supported_extensions(false, false), Vec::<String>::new());
        assert_eq!(
            supported_extensions(true, false),
            vec!["eip2612GasSponsoring"]
        );
        assert_eq!(supported_extensions(false, true), vec!["builder-code"]);
        assert_eq!(
            supported_extensions(true, true),
            vec!["eip2612GasSponsoring", "builder-code"]
        );
    }
}
