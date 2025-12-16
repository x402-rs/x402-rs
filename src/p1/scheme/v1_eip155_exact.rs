use crate::chain::FacilitatorLocalError;
use crate::p1::chain::ChainProvider;
use crate::p1::scheme::{SchemeSlug, X402SchemeBlueprint, X402SchemeHandler};
use crate::types::SupportedResponse;
use std::sync::Arc;

pub struct V1Eip155Exact;

impl X402SchemeBlueprint for V1Eip155Exact {
    fn slug(&self) -> SchemeSlug {
        SchemeSlug::new(1, "eip155", "exact")
    }

    fn build(
        &self,
        provider: Arc<ChainProvider>,
    ) -> Result<Box<dyn X402SchemeHandler>, Box<dyn std::error::Error>> {
        let provider = if let ChainProvider::Eip155(provider) = provider.as_ref() {
            provider
        } else {
            return Err("V1Eip155Exact::build: provider must be an Eip155ChainProvider".into());
        };
        Ok(Box::new(V1Eip155ExactHandler {}))
    }
}

pub struct V1Eip155ExactHandler {}

#[async_trait::async_trait]
impl X402SchemeHandler for V1Eip155ExactHandler {
    async fn supported(&self) -> Result<SupportedResponse, FacilitatorLocalError> {
        todo!("V1Eip155ExactHandler::supported")
    }
}
