//! Server-side price requirements for native BCH exact payments.

use std::sync::Arc;
use x402_types::chain::ChainId;
use x402_types::proto;
use x402_types::proto::v2;

use crate::address::CashAddr;
use crate::chain::BchChainReference;
use crate::transaction::BchPolicy;
use crate::v2_bch_exact::V2BchExact;
use crate::v2_bch_exact::types::{BchExtra, ExactScheme};

pub type BchPaymentRequirements = v2::PaymentRequirements;

impl V2BchExact {
    pub fn price_tag(
        pay_to: impl Into<String>,
        amount: u64,
        network: BchChainReference,
    ) -> v2::PriceTag {
        v2::PriceTag {
            requirements: v2::PaymentRequirements {
                scheme: ExactScheme.to_string(),
                network: ChainId::from(network),
                amount: amount.to_string(),
                pay_to: pay_to.into(),
                max_timeout_seconds: 300,
                asset: "BCH".to_string(),
                extra: Some(
                    serde_json::to_value(BchExtra::default()).expect("BCH extra is serializable"),
                ),
            },
            enricher: Some(Arc::new(
                |price_tag: &mut v2::PriceTag, _supported: &proto::SupportedResponse| {
                    if price_tag.requirements.extra.is_none() {
                        price_tag.requirements.extra = Some(
                            serde_json::to_value(BchExtra::default())
                                .expect("BCH extra is serializable"),
                        );
                    }
                },
            )),
        }
    }

    pub fn cash_token_price_tag(
        pay_to: impl Into<String>,
        category: impl Into<String>,
        amount: u64,
        token_output_value: Option<u64>,
        network: BchChainReference,
    ) -> v2::PriceTag {
        let category = category.into();
        let extra = BchExtra {
            asset_transfer_method: "cashtoken".to_string(),
            payment_flow: "upfront".to_string(),
            token_output_value: Some(
                token_output_value
                    .unwrap_or(BchPolicy::default().dust_threshold)
                    .to_string(),
            ),
            token: None,
        };
        v2::PriceTag {
            requirements: v2::PaymentRequirements {
                scheme: ExactScheme.to_string(),
                network: ChainId::from(network),
                amount: amount.to_string(),
                pay_to: pay_to.into(),
                max_timeout_seconds: 300,
                asset: category,
                extra: Some(serde_json::to_value(&extra).expect("BCH extra is serializable")),
            },
            enricher: Some(Arc::new(move |price_tag: &mut v2::PriceTag, _supported| {
                if price_tag.requirements.extra.is_none() {
                    price_tag.requirements.extra =
                        Some(serde_json::to_value(&extra).expect("BCH extra is serializable"));
                }
            })),
        }
    }

    pub fn validate_price_tag(
        requirements: &v2::PaymentRequirements,
    ) -> Result<(), crate::address::CashAddrError> {
        let network = BchChainReference::try_from(requirements.network.clone()).map_err(|_| {
            crate::address::CashAddrError::UnsupportedPrefix(requirements.network.to_string())
        })?;
        CashAddr::decode_script(&requirements.pay_to, network).map(|_| ())
    }
}
