use alloy_signer::Signer;
use x402_rs::scheme::X402SchemeId;

use crate::client::X402SchemeClient;

pub struct V2Eip155ExactClient<S> {
    signer: S,
}

impl<S> X402SchemeId for V2Eip155ExactClient<S> {
    fn namespace(&self) -> &str {
        "eip155"
    }

    fn scheme(&self) -> &str {
        "exact"
    }
}

impl<S> V2Eip155ExactClient<S> {
    pub fn new(signer: S) -> Self {
        Self { signer }
    }
}

impl<S> From<S> for V2Eip155ExactClient<S>
where
    S: Signer + Send + Sync,
{
    fn from(signer: S) -> Self {
        Self::new(signer)
    }
}

impl<S> X402SchemeClient for V2Eip155ExactClient<S> where S: Signer + Send + Sync {}
