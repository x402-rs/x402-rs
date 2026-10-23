use alloy_provider::Provider;
use alloy_sol_types::Eip712Domain;
use x402_types::chain::{ChainId, ChainProviderOps};
use x402_types::proto::{PaymentVerificationError, v2};
use x402_types::scheme::X402SchemeFacilitatorError;

#[cfg(feature = "telemetry")]
use tracing::instrument;

use crate::chain::settle_request_hash;
use crate::chain::{Eip155ChainReference, Eip155MetaTransactionProvider, Eip3009Authorization};
use crate::v1_eip155_exact::{
    Eip155ExactError, ExactEvmPayment, IEIP3009, PaymentRequirementsExtra, assert_domain,
    assert_enough_balance, assert_enough_value, assert_time, settle_or_find_used_authorization,
    settle_payment, settle_with_pending_store, verify_payment,
};
use crate::v2_eip155_exact::Eip3009Payload;
use crate::v2_eip155_exact::types::{Eip3009PaymentPayload, Eip3009PaymentRequirements};

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn verify_eip3009_payment<P: Eip155MetaTransactionProvider + ChainProviderOps>(
    provider: &P,
    payment_payload: &Eip3009PaymentPayload,
    payment_requirements: &Eip3009PaymentRequirements,
) -> Result<v2::VerifyResponse, X402SchemeFacilitatorError> {
    let accepted = &payment_payload.accepted;
    assert_requirements_match(accepted, payment_requirements)?;
    let (contract, payment, eip712_domain) = assert_valid_payment(
        provider.inner(),
        provider.chain(),
        accepted,
        &payment_payload.payload,
    )
    .await?;

    let payer = verify_payment(provider.inner(), &contract, &payment, &eip712_domain).await?;
    Ok(v2::VerifyResponse::valid(payer.to_string()))
}

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn settle_eip3009_payment<P>(
    provider: &P,
    payment_payload: &Eip3009PaymentPayload,
    payment_requirements: &Eip3009PaymentRequirements,
) -> Result<v2::SettleResponse, X402SchemeFacilitatorError>
where
    P: Eip155MetaTransactionProvider + ChainProviderOps,
    Eip155ExactError: From<P::Error>,
{
    let accepted = &payment_payload.accepted;
    assert_requirements_match(accepted, payment_requirements)?;
    let network = accepted.network.to_string();
    let authorization =
        authorization_to_reconcile(provider.chain(), accepted, &payment_payload.payload);
    let tx_hash = settle_with_pending_store(
        provider,
        &payment_payload.payload.signature,
        settle_request_hash(payment_payload, payment_requirements),
        &network,
        || {
            settle_or_find_used_authorization(provider, authorization, async {
                let (contract, payment, eip712_domain) = assert_valid_payment(
                    provider.inner(),
                    provider.chain(),
                    accepted,
                    &payment_payload.payload,
                )
                .await?;
                settle_payment(provider, &contract, &payment, &eip712_domain).await
            })
        },
    )
    .await?;

    Ok(v2::SettleResponse::Success {
        payer: payment_payload.payload.authorization.from.to_string(),
        transaction: tx_hash.to_string(),
        network,
    })
}

/// Runs all preconditions needed for a successful payment:
/// - Valid scheme, network, and receiver.
/// - Valid time window (validAfter/validBefore).
/// - Correct EIP-712 domain construction.
/// - Sufficient on-chain balance.
/// - Sufficient value in payload.
#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn assert_valid_payment<P: Provider>(
    provider: P,
    chain: &Eip155ChainReference,
    accepted: &Eip3009PaymentRequirements,
    payload: &Eip3009Payload,
) -> Result<(IEIP3009::IEIP3009Instance<P>, ExactEvmPayment, Eip712Domain), Eip155ExactError> {
    let chain_id: ChainId = chain.into();
    let payload_chain_id = &accepted.network;
    if payload_chain_id != &chain_id {
        return Err(PaymentVerificationError::ChainIdMismatch.into());
    }
    let authorization = &payload.authorization;
    if authorization.to != accepted.pay_to {
        return Err(PaymentVerificationError::RecipientMismatch.into());
    }
    let valid_after = authorization.valid_after;
    let valid_before = authorization.valid_before;
    assert_time(valid_after, valid_before)?;
    let asset_address = accepted.asset;
    let contract = IEIP3009::new(asset_address.into(), provider);

    let amount_required = accepted.amount;
    assert_enough_value(&authorization.value, &amount_required)?;

    let extra = Some(PaymentRequirementsExtra {
        name: accepted.extra.name.clone(),
        version: accepted.extra.version.clone(),
    });
    let domain = assert_domain(chain, &contract, &asset_address.into(), &extra).await?;

    assert_enough_balance(&contract, &authorization.from, amount_required).await?;

    let signature = payload.signature.clone();

    let payment = ExactEvmPayment {
        from: authorization.from,
        to: authorization.to,
        value: authorization.value,
        valid_after: authorization.valid_after,
        valid_before: authorization.valid_before,
        nonce: authorization.nonce,
        signature,
    };

    Ok((contract, payment, domain))
}

/// The authorization of a settle request, to look up on chain if settling it fails, when the
/// request matches its requirements on `chain` (network, recipient and amount). `None` for a
/// request that does not: it fails as before, without a lookup.
fn authorization_to_reconcile(
    chain: &Eip155ChainReference,
    accepted: &Eip3009PaymentRequirements,
    payload: &Eip3009Payload,
) -> Option<Eip3009Authorization> {
    let chain_id: ChainId = chain.into();
    let authorization = &payload.authorization;
    let matches = accepted.network == chain_id
        && authorization.to == accepted.pay_to
        && authorization.value >= accepted.amount;
    matches.then(|| Eip3009Authorization::new(accepted.asset.into(), authorization))
}

pub fn assert_requirements_match<T: PartialEq>(
    accepted: &T,
    payment_requirements: &T,
) -> Result<(), PaymentVerificationError> {
    if accepted != payment_requirements {
        Err(PaymentVerificationError::AcceptedRequirementsMismatch)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, B256, U256, address, b256};
    use x402_types::timestamp::UnixTimestamp;

    const ASSET: Address = address!("0x036CbD53842c5426634e7929541eC2318f3dCF7e");
    const PAYER: Address = address!("0x1111111111111111111111111111111111111111");
    const SELLER: Address = address!("0x2222222222222222222222222222222222222222");
    const NONCE: B256 = b256!("0x6666666666666666666666666666666666666666666666666666666666666666");

    fn request(
        pay_to: Address,
        amount: u64,
        network: &str,
    ) -> (Eip3009PaymentRequirements, Eip3009Payload) {
        let accepted = serde_json::from_value(serde_json::json!({
            "scheme": "exact",
            "network": network,
            "amount": amount.to_string(),
            "payTo": pay_to,
            "maxTimeoutSeconds": 60,
            "asset": ASSET,
            "extra": {"name": "USDC", "version": "2"}
        }))
        .unwrap();
        let payload = serde_json::from_value(serde_json::json!({
            "signature": "0x1234",
            "authorization": {
                "from": PAYER,
                "to": SELLER,
                "value": "10000",
                "validAfter": "1000",
                "validBefore": "2000",
                "nonce": NONCE
            }
        }))
        .unwrap();
        (accepted, payload)
    }

    #[test]
    fn test_request_is_looked_up_only_when_it_matches_its_requirements() {
        let chain = Eip155ChainReference::new(84532);
        let (accepted, payload) = request(SELLER, 10_000, "eip155:84532");
        assert_eq!(
            authorization_to_reconcile(&chain, &accepted, &payload),
            Some(Eip3009Authorization {
                asset: ASSET,
                from: PAYER,
                to: SELLER,
                value: U256::from(10_000u64),
                valid_after: UnixTimestamp::from_secs(1_000),
                valid_before: UnixTimestamp::from_secs(2_000),
                nonce: NONCE,
            })
        );
        for (pay_to, amount, network) in [
            (
                address!("0x9999999999999999999999999999999999999999"),
                10_000,
                "eip155:84532",
            ),
            (SELLER, 10_001, "eip155:84532"),
            (SELLER, 10_000, "eip155:8453"),
        ] {
            let (accepted, payload) = request(pay_to, amount, network);
            assert_eq!(
                authorization_to_reconcile(&chain, &accepted, &payload),
                None,
                "{pay_to} {amount} {network}"
            );
        }
    }
}
