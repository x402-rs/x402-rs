use std::str::FromStr;
use alloy_primitives::U256;
use x402_rs::chain::ChainId;
use x402_rs::proto::{v1, v2};
use x402_rs::proto::client::PaymentCandidateLike;
use x402_rs::util::Base64Bytes;

use crate::client::ClientSchemes;

#[derive(Debug)]
pub enum PaymentRequired {
    V1(v1::PaymentRequired),
    V2(v2::PaymentRequired),
}

pub struct PaymentCandidateB<'a> {
    chain_id: ChainId,
    asset: &'a str,
    amount: U256,
    scheme: &'a str,
    x402_version: u8,
}

impl PaymentCandidateLike for PaymentCandidateB<'_> {
    fn chain_id(&self) -> &ChainId { &self.chain_id }
    fn asset(&self) -> &str { self.asset }
    fn amount(&self) -> U256 { self.amount }
    fn scheme(&self) -> &str { self.scheme }
    fn x402_version(&self) -> u8 { self.x402_version }
}

impl PaymentRequired {
    pub async fn from_response(response: reqwest::Response) -> Option<Self> {
        let headers = response.headers();
        let v2_payment_required = headers
            .get("Payment-Required")
            .and_then(|h| Base64Bytes::from(h.as_bytes()).decode().ok())
            .and_then(|b| serde_json::from_slice::<v2::PaymentRequired>(&b).ok());
        if let Some(v2_payment_required) = v2_payment_required {
            return Some(Self::V2(v2_payment_required));
        }
        let v1_payment_required = response.json::<v1::PaymentRequired>().await.ok();
        if let Some(v1_payment_required) = v1_payment_required {
            return Some(Self::V1(v1_payment_required));
        }
        None
    }

    // pub fn candidates(&self, schemes: &ClientSchemes) {
    //     match self {
    //         PaymentRequired::V1(_) => {
    //             todo!()
    //         }
    //         PaymentRequired::V2(payment_required) => {
    //             for raw in &payment_required.accepts {
    //                 let scheme = raw.get("scheme").and_then(|v| v.as_str()).unwrap_or("");
    //                 let chain_id = raw.get("network").and_then(|v| v.as_str()).and_then(|s| ChainId::from_str(s).ok());
    //                 let chain_id = match chain_id {
    //                     Some(chain_id) => chain_id,
    //                     None => continue, // Skip invalid network formats
    //                 };
    //                 let resource = &payment_required.resource;
    //                 for registered in schemes.iter() {
    //                     if registered.matches(2, scheme, &chain_id) {
    //                         let candidate = registered.client().build_candidate(raw, resource);
    //                         println!("Found candidate: scheme={}, network={:?}", scheme, chain_id);
    //                         println!("candidate {:?}", candidate);
    //                         // let candidate = PaymentCandidateB {
    //                         //     chain_id,
    //                         //     asset: "",
    //                         //     amount: U256::zero(),
    //                         //     scheme,
    //                         //     x402_version: 2,
    //                         // };
    //                     }
    //                 }
    //                 println!("Found candidate: scheme={}, network={:?}", scheme, chain_id);
    //             }
    //         }
    //     }
    // }
}
