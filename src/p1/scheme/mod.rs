pub mod v1_eip155_exact;

pub use v1_eip155_exact::V1Eip155Exact;

use crate::p1::chain::ChainProvider;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::str::FromStr;
use std::sync::Arc;

pub trait X402SchemeHandler {}

pub trait X402SchemeBlueprint {
    fn slug(&self) -> SchemeSlug;
    fn build(
        &self,
        provider: Arc<ChainProvider>,
    ) -> Result<Box<dyn X402SchemeHandler>, Box<dyn std::error::Error>>;
}

#[derive(Default)]
pub struct SchemeBlueprints(HashMap<SchemeSlug, Box<dyn X402SchemeBlueprint>>);

impl Debug for SchemeBlueprints {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let slugs: Vec<String> = self.0.keys().map(|s| s.to_string()).collect();
        f.debug_struct("SchemeRegistry")
            .field("schemes", &slugs)
            .finish()
    }
}

impl SchemeBlueprints {
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

#[derive(Debug, thiserror::Error)]
pub enum SchemeSlugError {
    #[error("invalid scheme slug format: {0}")]
    InvalidFormat(String),
    #[error("invalid version format: expected 'v<number>', got: {0}")]
    InvalidVersion(String),
}

impl FromStr for SchemeSlug {
    type Err = SchemeSlugError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // Expected format: v{version}:{namespace}:{name}
        let parts: Vec<&str> = s.splitn(3, ':').collect();
        if parts.len() != 3 {
            return Err(SchemeSlugError::InvalidFormat(s.to_string()));
        }

        let version_str = parts[0];
        if !version_str.starts_with('v') {
            return Err(SchemeSlugError::InvalidVersion(version_str.to_string()));
        }

        let x402_version: u8 = version_str[1..]
            .parse()
            .map_err(|_| SchemeSlugError::InvalidVersion(version_str.to_string()))?;

        Ok(SchemeSlug {
            x402_version,
            namespace: parts[1].to_string(),
            name: parts[2].to_string(),
        })
    }
}

impl Serialize for SchemeSlug {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SchemeSlug {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        SchemeSlug::from_str(&s).map_err(serde::de::Error::custom)
    }
}