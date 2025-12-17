use std::error::Error;
use std::sync::Arc;

use crate::chain::ChainProvider;
use crate::facilitator_local::FacilitatorLocalError;
use crate::proto::{
    SettleRequest, SettleResponse, SupportedResponse, VerifyRequest, VerifyResponse,
};
use crate::scheme::{SchemeSlug, X402SchemeBlueprint, X402SchemeHandler, v1_eip155_exact};
use crate::chain::eip155::Eip155ChainProvider;

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
        request: &VerifyRequest,
    ) -> Result<VerifyResponse, FacilitatorLocalError> {
        todo!()
    }

    async fn settle(
        &self,
        request: &SettleRequest,
    ) -> Result<SettleResponse, FacilitatorLocalError> {
        todo!()
    }

    async fn supported(&self) -> Result<SupportedResponse, FacilitatorLocalError> {
        todo!()
    }
}
