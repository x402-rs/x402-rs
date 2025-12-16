pub mod v1_eip155_exact;

pub use v1_eip155_exact::V1Eip155Exact;

use std::collections::HashMap;
use std::fmt::{Debug, Formatter};

use crate::p1::chain::ChainProvider;

pub trait X402SchemeHandler {}

pub trait X402SchemeBlueprint {
    fn slug(&self) -> SchemeSlug;
    fn build(
        &self,
        provider: ChainProvider,
    ) -> Result<Box<dyn X402SchemeHandler>, Box<dyn std::error::Error>>;
}

#[derive(Default)]
pub struct SchemeRegistry(HashMap<SchemeSlug, Box<dyn X402SchemeBlueprint>>);

impl Debug for SchemeRegistry {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let slugs: Vec<String> = self.0.keys().map(|s| s.to_string()).collect();
        f.debug_struct("SchemeRegistry")
            .field("schemes", &slugs)
            .finish()
    }
}

impl SchemeRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn and_register<B: X402SchemeBlueprint + 'static>(&mut self, blueprint: B) -> &mut Self {
        self.register(blueprint);
        self
    }

    pub fn register<B: X402SchemeBlueprint + 'static>(&mut self, blueprint: B) {
        self.0.insert(blueprint.slug(), Box::new(blueprint));
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SchemeSlug {
    x402_version: u8,
    namespace: String,
    name: String,
}

impl SchemeSlug {
    pub fn new<N: Into<String>, M: Into<String>>(x402_version: u8, namespace: N, name: M) -> Self {
        Self {
            x402_version,
            namespace: namespace.into(),
            name: name.into(),
        }
    }
}

impl std::fmt::Display for SchemeSlug {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}:{}:{}", self.x402_version, self.namespace, self.name)
    }
}
