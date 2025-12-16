use crate::p1::chain::ChainProvider;
use crate::p1::scheme::{SchemeSlug, X402SchemeHandler, X402SchemeBlueprint};

pub struct V1Eip155Exact;

impl X402SchemeBlueprint for V1Eip155Exact {
    fn slug(&self) -> SchemeSlug {
        SchemeSlug::new(1, "eip155", "exact")
    }

    fn build(
        &self,
        provider: ChainProvider,
    ) -> Result<Box<dyn X402SchemeHandler>, Box<dyn std::error::Error>> {
        todo!()
    }
}

pub struct V1Eip155ExactHandler {}

impl X402SchemeHandler for V1Eip155ExactHandler {}