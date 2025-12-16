use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;
use serde::{Deserialize, Serialize};
use crate::chain::FacilitatorLocalError;
use crate::p1::chain::{ChainProvider, ChainProviderOps};
use crate::p1::chain::solana::{Address, SolanaChainProvider};
use crate::p1::proto;
use crate::p1::scheme::{X402SchemeBlueprint, X402SchemeHandler};

const SCHEME_NAME: &str = "exact";

pub struct V1SolanaExact;

impl X402SchemeBlueprint for V1SolanaExact {
    fn slug(&self) -> crate::p1::scheme::SchemeSlug {
        crate::p1::scheme::SchemeSlug::new(1, "solana", SCHEME_NAME)
    }

    fn build(&self, provider: ChainProvider) -> Result<Box<dyn X402SchemeHandler>, Box<dyn Error>> {
        let provider = if let ChainProvider::Solana(provider) = provider {
            provider
        } else {
            return Err("V1SolanaExact::build: provider must be a SolanaChainProvider".into());
        };
        Ok(Box::new(V1SolanaExactHandler { provider }))
    }
}

pub struct V1SolanaExactHandler {
    provider: Arc<SolanaChainProvider>,
}

#[async_trait::async_trait]
impl X402SchemeHandler for V1SolanaExactHandler {
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, FacilitatorLocalError> {
        todo!("V1SolanaExactHandler::verify")
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, FacilitatorLocalError> {
        todo!("V1SolanaExactHandler::settle")
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, FacilitatorLocalError> {
        let chain_id = self.provider.chain_id();
        let kinds: Vec<proto::SupportedPaymentKind> = {
            let mut kinds = Vec::with_capacity(2);
            let fee_payer = self.provider.fee_payer();
            let extra =
                Some(serde_json::to_value(SupportedPaymentKindExtra { fee_payer }).unwrap());
            kinds.push(proto::SupportedPaymentKind {
                x402_version: proto::X402Version::v2().into(),
                scheme: SCHEME_NAME.to_string(),
                network: chain_id.clone().to_string(),
                extra: extra.clone(),
            });
            let network = chain_id.as_network_name();
            if let Some(network) = network {
                kinds.push(proto::SupportedPaymentKind {
                    x402_version: proto::X402Version::v2().into(),
                    scheme: SCHEME_NAME.to_string(),
                    network: network.to_string(),
                    extra,
                });
            }
            kinds
        };
        let signers = {
            let mut signers = HashMap::with_capacity(1);
            signers.insert(chain_id, self.provider.signer_addresses());
            signers
        };
        Ok(proto::SupportedResponse {
            kinds,
            extensions: Vec::new(),
            signers,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SupportedPaymentKindExtra {
    fee_payer: Address,
}