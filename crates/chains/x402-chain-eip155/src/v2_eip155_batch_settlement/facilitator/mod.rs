//! Facilitator-side implementation of the V2 EIP-155 `batch-settlement` scheme.
//!
//! The facilitator implements three operations:
//!
//! - [`X402SchemeFacilitator::verify`]   — validate a payment payload (deposit,
//!   voucher, or refund) without committing onchain. Returns the onchain
//!   channel snapshot so the server can keep its mirrored state fresh.
//! - [`X402SchemeFacilitator::settle`]   — execute one of four settle actions
//!   (deposit, claim, settle, refund) onchain. Surfaces the transaction hash
//!   and a post-tx channel snapshot.
//! - [`X402SchemeFacilitator::supported`] — advertise the scheme and the
//!   facilitator's `receiverAuthorizer` address (when configured) so servers
//!   can delegate authorizer signing to the facilitator.
//!
//! Configuration:
//!
//! ```json
//! {
//!   "id": "v2-eip155-batch-settlement",
//!   "chains": "eip155:*",
//!   "config": {
//!     "receiverAuthorizerPrivateKey": "0x...",
//!     "eip2612GasSponsoring": false
//!   }
//! }
//! ```
//!
//! - `receiverAuthorizerPrivateKey` is optional. When set, the facilitator
//!   exposes the derived EOA as `extra.receiverAuthorizer` in
//!   `SupportedResponse.kinds[].extra` and signs missing claim / refund
//!   authorizer signatures with it. When unset, servers must hold their
//!   own receiver-authorizer key and supply signatures inline.
//! - `eip2612GasSponsoring` is reserved for the future EIP-2612 inline-permit
//!   deposit branch. The baseline port relays Permit2 deposits with the
//!   standard branch only.

pub mod abi;
pub mod authorizer_signer;
pub mod response;
pub mod settle;
pub mod utils;
pub mod verify;
pub mod voucher;

pub use authorizer_signer::ReceiverAuthorizerSigner;
pub use response::{
    BatchSettlementSettleExtra, BatchSettlementSettleResponse, BatchSettlementVerifyExtra,
    BatchSettlementVerifyResponse,
};
pub use utils::{OnchainChannelState, compute_channel_id, compute_voucher_digest};

use alloy_provider::Provider;
use alloy_signer_local::PrivateKeySigner;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use x402_types::chain::ChainProviderOps;
use x402_types::proto;
use x402_types::proto::v2;
use x402_types::scheme::{
    X402SchemeFacilitator, X402SchemeFacilitatorBuilder, X402SchemeFacilitatorError,
};

use crate::V2Eip155BatchSettlement;
use crate::chain::{Eip155ChainReference, Eip155MetaTransactionProvider, MetaTransactionSendError};
use crate::v2_eip155_batch_settlement::constants::BATCH_SETTLEMENT_SCHEME;
use crate::v2_eip155_batch_settlement::types as wire;

/// JSON-configured options for the batch-settlement facilitator.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct V2Eip155BatchSettlementConfig {
    /// Hex-encoded private key (0x-prefixed) for the facilitator's
    /// receiver authorizer EOA. When unset, the facilitator does not
    /// advertise a `receiverAuthorizer` in `supported()` and cannot
    /// sign missing claim / refund authorizer signatures.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub receiver_authorizer_private_key: Option<String>,
    /// Reserved: enable the EIP-2612 inline-permit branch for Permit2 deposits.
    /// The baseline port does not implement this branch; setting `true`
    /// today has no effect.
    #[serde(default)]
    pub eip2612_gas_sponsoring: bool,
}

impl<P> X402SchemeFacilitatorBuilder<P> for V2Eip155BatchSettlement
where
    P: Eip155MetaTransactionProvider + ChainProviderOps + Send + Sync + 'static,
    P::Error: Into<MetaTransactionSendError>,
{
    fn build(
        &self,
        provider: P,
        config: Option<serde_json::Value>,
    ) -> Result<Box<dyn X402SchemeFacilitator>, Box<dyn std::error::Error>> {
        let config: V2Eip155BatchSettlementConfig = match config {
            Some(value) => serde_json::from_value(value)?,
            None => V2Eip155BatchSettlementConfig::default(),
        };

        let receiver_authorizer = match &config.receiver_authorizer_private_key {
            Some(hex) => {
                let signer: PrivateKeySigner =
                    hex.parse().map_err(|e| -> Box<dyn std::error::Error> {
                        format!("invalid receiverAuthorizerPrivateKey: {e}").into()
                    })?;
                Some(ReceiverAuthorizerSigner::new(signer))
            }
            None => None,
        };

        Ok(Box::new(V2Eip155BatchSettlementFacilitator {
            provider,
            receiver_authorizer,
        }))
    }
}

/// Facilitator implementation for V2 EIP-155 `batch-settlement`.
///
/// Decoupled from any single provider implementation — accepts anything that
/// implements [`Eip155MetaTransactionProvider`] + [`ChainProviderOps`], so it
/// can be exercised with a mock provider in tests.
pub struct V2Eip155BatchSettlementFacilitator<P> {
    provider: P,
    receiver_authorizer: Option<ReceiverAuthorizerSigner>,
}

impl<P> V2Eip155BatchSettlementFacilitator<P> {
    /// Constructs a facilitator directly (bypassing JSON config).
    pub fn new(provider: P, receiver_authorizer: Option<ReceiverAuthorizerSigner>) -> Self {
        Self {
            provider,
            receiver_authorizer,
        }
    }
}

#[async_trait::async_trait]
impl<P> X402SchemeFacilitator for V2Eip155BatchSettlementFacilitator<P>
where
    P: Eip155MetaTransactionProvider + ChainProviderOps + Send + Sync,
    P::Inner: Provider,
    P::Error: Into<MetaTransactionSendError>,
{
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, X402SchemeFacilitatorError> {
        let typed: wire::VerifyRequest = wire::VerifyRequest::try_from(request)?;
        let chain_id = chain_id_u64(self.provider.chain());
        let response = verify::verify(
            self.provider.inner(),
            chain_id,
            &typed.payment_payload,
            &typed.payment_requirements,
        )
        .await;
        Ok(response.into())
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, X402SchemeFacilitatorError> {
        let typed: wire::SettleRequest = wire::SettleRequest::try_from(request)?;
        let chain_id = chain_id_u64(self.provider.chain());
        let response = settle::settle(
            &self.provider,
            chain_id,
            self.receiver_authorizer.as_ref(),
            &typed.payment_payload,
            &typed.payment_requirements,
        )
        .await;
        Ok(response.into())
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, X402SchemeFacilitatorError> {
        let chain_id = self.provider.chain_id();
        let extra = self.receiver_authorizer.as_ref().map(|signer| {
            serde_json::json!({
                "receiverAuthorizer": signer.address().to_checksum(None),
            })
        });
        let kinds = vec![proto::SupportedPaymentKind {
            x402_version: v2::X402Version2.into(),
            scheme: BATCH_SETTLEMENT_SCHEME.to_string(),
            network: chain_id.clone().into(),
            extra,
        }];
        let mut signers = HashMap::with_capacity(1);
        signers.insert(chain_id, self.provider.signer_addresses());
        Ok(proto::SupportedResponse {
            kinds,
            extensions: Vec::new(),
            signers,
        })
    }
}

fn chain_id_u64(chain: &Eip155ChainReference) -> u64 {
    chain.inner()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_signer_local::PrivateKeySigner;

    #[test]
    fn config_parses_with_authorizer_key() {
        let signer = PrivateKeySigner::random();
        let hex = format!("{:#x}", signer.to_bytes());
        let json = serde_json::json!({
            "receiverAuthorizerPrivateKey": hex,
            "eip2612GasSponsoring": false,
        });
        let parsed: V2Eip155BatchSettlementConfig = serde_json::from_value(json).unwrap();
        assert!(parsed.receiver_authorizer_private_key.is_some());
        assert!(!parsed.eip2612_gas_sponsoring);
    }

    #[test]
    fn config_defaults_round_trip() {
        let json = serde_json::json!({});
        let parsed: V2Eip155BatchSettlementConfig = serde_json::from_value(json).unwrap();
        assert!(parsed.receiver_authorizer_private_key.is_none());
        assert!(!parsed.eip2612_gas_sponsoring);
    }
}
