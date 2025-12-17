use std::collections::HashMap;
use std::error::Error;
use std::sync::Arc;

use crate::chain::eip155::Eip155ChainProvider;
use crate::chain::{ChainProvider, ChainProviderOps};
use crate::facilitator_local::FacilitatorLocalError;
use crate::proto;
use crate::scheme::{SchemeSlug, X402SchemeBlueprint, X402SchemeHandler, v1_eip155_exact};

const EXACT_SCHEME: v1_eip155_exact::types::ExactScheme =
    v1_eip155_exact::types::ExactScheme::Exact;

pub struct V2Eip155Exact;

impl X402SchemeBlueprint for V2Eip155Exact {
    fn slug(&self) -> SchemeSlug {
        SchemeSlug::new(2, "eip155", EXACT_SCHEME.to_string())
    }

    fn build(&self, provider: ChainProvider) -> Result<Box<dyn X402SchemeHandler>, Box<dyn Error>> {
        let provider = if let ChainProvider::Eip155(provider) = provider {
            provider
        } else {
            return Err("V2Eip155Exact::build: provider must be an Eip155ChainProvider".into());
        };
        Ok(Box::new(V2Eip155ExactHandler { provider }))
    }
}

pub struct V2Eip155ExactHandler {
    provider: Arc<Eip155ChainProvider>,
}

#[async_trait::async_trait]
impl X402SchemeHandler for V2Eip155ExactHandler {
    async fn verify(
        &self,
        request: &proto::VerifyRequest,
    ) -> Result<proto::VerifyResponse, FacilitatorLocalError> {
        todo!("V2Eip155ExactHandler::verify: not implemented yet")
    }

    async fn settle(
        &self,
        request: &proto::SettleRequest,
    ) -> Result<proto::SettleResponse, FacilitatorLocalError> {
        todo!()
    }

    async fn supported(&self) -> Result<proto::SupportedResponse, FacilitatorLocalError> {
        let chain_id = self.provider.chain_id();
        let kinds = {
            let mut kinds = Vec::with_capacity(1);
            kinds.push(proto::SupportedPaymentKind {
                x402_version: proto::X402Version::v2().into(),
                scheme: EXACT_SCHEME.to_string(),
                network: chain_id.clone().into(),
                extra: None,
            });
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
