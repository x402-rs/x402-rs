use std::sync::Arc;

use crate::p1::chain::ChainProvider;
use crate::p1::scheme::{SchemeSlug, X402SchemeBlueprint, X402SchemeHandler};

pub struct V1Eip155Exact;

impl X402SchemeBlueprint for V1Eip155Exact {
    fn slug(&self) -> SchemeSlug {
        SchemeSlug::new(1, "eip155", "exact")
    }

    fn build(
        &self,
        provider: Arc<ChainProvider>,
    ) -> Result<Box<dyn X402SchemeHandler>, Box<dyn std::error::Error>> {
        Ok(Box::new(V1Eip155ExactHandler{}))
    }
}

pub struct V1Eip155ExactHandler {}

impl X402SchemeHandler for V1Eip155ExactHandler {}
