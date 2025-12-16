use std::fmt::{Display, Formatter};

use crate::config::Eip155ChainConfig;
use crate::p1::chain::{ChainId, ChainIdError, ChainProviderOps};

pub const EIP155_NAMESPACE: &str = "eip155";

#[derive(Debug, Copy, Clone)]
pub struct Eip155ChainReference(u64);

impl Into<ChainId> for Eip155ChainReference {
    fn into(self) -> ChainId {
        ChainId::new(EIP155_NAMESPACE, self.0.to_string())
    }
}

impl Into<ChainId> for &Eip155ChainReference {
    fn into(self) -> ChainId {
        ChainId::new(EIP155_NAMESPACE, self.0.to_string())
    }
}

impl TryFrom<ChainId> for Eip155ChainReference {
    type Error = ChainIdError;

    fn try_from(value: ChainId) -> Result<Self, Self::Error> {
        if value.namespace != EIP155_NAMESPACE {
            return Err(ChainIdError::UnexpectedNamespace(
                value.namespace,
                EIP155_NAMESPACE.into(),
            ));
        }
        let chain_id: u64 = value.reference.parse().map_err(|e| {
            ChainIdError::InvalidReference(
                value.reference,
                EIP155_NAMESPACE.into(),
                format!("{e:?}").into(),
            )
        })?;
        Ok(Eip155ChainReference(chain_id))
    }
}

impl Eip155ChainReference {
    pub fn new(chain_id: u64) -> Self {
        Self(chain_id)
    }
    pub fn inner(&self) -> u64 {
        self.0
    }
}

impl Display for Eip155ChainReference {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug)]
pub struct Eip155ChainProvider {
    chain: Eip155ChainReference,
}

impl Eip155ChainProvider {
    pub async fn from_config(
        config: &Eip155ChainConfig,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            chain: config.chain_reference(),
        })
    }
}

impl ChainProviderOps for Eip155ChainProvider {
    fn signer_addresses(&self) -> Vec<String> {
        // FIXME TODO
        vec![]
    }

    fn chain_id(&self) -> ChainId {
        self.chain.into()
    }
}
