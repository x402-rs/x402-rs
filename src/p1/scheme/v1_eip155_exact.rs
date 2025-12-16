use crate::p1::chain::ChainProvider;
use crate::p1::scheme::{SchemeSlug, X402Scheme, X402SchemeBlueprint};

pub struct V1Eip155ExactScheme {}

impl V1Eip155ExactScheme {
    pub fn blueprint() -> V1Eip155ExactSchemeBlueprint {
        V1Eip155ExactSchemeBlueprint
    }
}

pub struct V1Eip155ExactSchemeBlueprint;

impl X402Scheme for V1Eip155ExactScheme {}

impl X402SchemeBlueprint for V1Eip155ExactSchemeBlueprint {
    fn slug(&self) -> SchemeSlug {
        SchemeSlug::new(1, "eip155", "exact")
    }

    fn build(
        &self,
        provider: ChainProvider,
    ) -> Result<Box<dyn X402Scheme>, Box<dyn std::error::Error>> {
        todo!()
    }
}
