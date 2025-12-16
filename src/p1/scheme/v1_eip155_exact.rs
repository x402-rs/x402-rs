use std::collections::HashMap;
use crate::chain::FacilitatorLocalError;
use crate::p1::proto;
use crate::p1::chain::{ChainId, ChainProvider, ChainProviderOps};
use crate::p1::chain::eip155;
use crate::p1::scheme::{SchemeSlug, X402SchemeBlueprint, X402SchemeHandler};
use crate::types::SupportedResponse;
use std::sync::Arc;
use crate::network::Network;

const SCHEME_NAME: &str = "exact";

pub struct V1Eip155Exact;

impl X402SchemeBlueprint for V1Eip155Exact {
    fn slug(&self) -> SchemeSlug {
        SchemeSlug::new(1, "eip155", SCHEME_NAME)
    }

    fn build(
        &self,
        provider: ChainProvider,
    ) -> Result<Box<dyn X402SchemeHandler>, Box<dyn std::error::Error>> {
        let provider = if let ChainProvider::Eip155(provider) = provider {
            provider
        } else {
            return Err("V1Eip155Exact::build: provider must be an Eip155ChainProvider".into());
        };
        Ok(Box::new(V1Eip155ExactHandler {
            provider
        }))
    }
}

pub struct V1Eip155ExactHandler {
    provider: Arc<eip155::Eip155ChainProvider>
}

impl V1Eip155ExactHandler {
    pub fn chain_id(&self) -> ChainId {
        self.provider.chain_id()
    }
}

#[async_trait::async_trait]
impl X402SchemeHandler for V1Eip155ExactHandler {
    async fn supported(&self) -> Result<proto::SupportedResponse, FacilitatorLocalError> {
        let kinds = {
            let mut kinds = Vec::with_capacity(2);
            kinds.push(proto::SupportedPaymentKind {
                x402_version: proto::X402Version::v2().into(),
                scheme: SCHEME_NAME.into(),
                network: self.chain_id().into(),
                extra: None,
            });
            let network: Option<Network> = self.chain_id().try_into().ok();
            if let Some(network) = network {
                kinds.push(proto::SupportedPaymentKind {
                    x402_version: proto::X402Version::v1().into(),
                    scheme: SCHEME_NAME.into(),
                    network: network.into(),
                    extra: None,
                });
            }
            kinds
        };
        let signers = {
            let mut signers = HashMap::with_capacity(1);
            signers.insert(self.chain_id(), self.provider.signer_addresses());
            signers
        };
        Ok(proto::SupportedResponse {
            kinds,
            extensions: Vec::new(),
            signers,
        })
    }
}
