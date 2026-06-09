//! EIP-2612 gas sponsoring extension facilitator logic for the upto scheme.
//!
//! When a client includes the `eip2612GasSponsoring` extension in its payment payload,
//! the facilitator:
//! 1. Verifies the EIP-2612 signature spender is the canonical Permit2 address.
//! 2. Simulates (verify) or executes (settle) `x402UptoPermit2Proxy.settleWithPermit`
//!    which atomically calls `IERC20Permit.permit` then Permit2 `permitWitnessTransferFrom`.

use alloy_primitives::{Address, Bytes, TxHash, U256};
use alloy_provider::{MulticallItem, Provider};
use x402_types::chain::ChainProviderOps;
use x402_types::timestamp::UnixTimestamp;

use super::types::{X402UptoPermit2Proxy, x402UptoPermit2Proxy};
use crate::chain::permit2::UPTO_PERMIT2_PROXY_ADDRESS;
use crate::chain::{Eip155MetaTransactionProvider, MetaTransaction};
use crate::eip2612_gas_sponsoring::{Eip2612GasSponsoring, Eip2612GasSponsoringInfo};
use crate::v1_eip155_exact::Eip155ExactError;
pub use crate::v2_eip155_exact::eip2612::Permit2PaymentPayloadExt;
use crate::v2_eip155_exact::facilitator::permit2::execute_permit2_settlement;
use crate::v2_eip155_upto::Permit2PaymentPayload;
use crate::v2_eip155_upto::facilitator::permit2::PreparedUptoPermit2;

#[cfg(feature = "telemetry")]
use tracing::Instrument;
#[cfg(feature = "telemetry")]
use tracing::instrument;

impl Permit2PaymentPayloadExt for Permit2PaymentPayload {
    fn eip2612_gas_sponsoring(&self) -> Option<Eip2612GasSponsoringInfo> {
        let eip2612_gas_sponsoring = self.extensions.get::<Eip2612GasSponsoring>()?;
        Some(eip2612_gas_sponsoring.info)
    }

    fn accepted_asset(&self) -> &Address {
        self.accepted.asset.as_ref()
    }

    fn authorization_from(&self) -> &Address {
        self.payload.permit_2_authorization.from.as_ref()
    }

    fn authorization_deadline(&self) -> &UnixTimestamp {
        &self.payload.permit_2_authorization.deadline
    }
}

/// Simulate `settleWithPermit` on-chain for upto payment verification.
///
/// Uses the full authorized amount (worst case) for the simulation.
#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn assert_onchain_upto_permit2_with_eip2612<P: Provider>(
    provider: &P,
    chain_reference: &crate::chain::Eip155ChainReference,
    payment_payload: &Permit2PaymentPayload,
    info: &Eip2612GasSponsoringInfo,
) -> Result<(), Eip155ExactError> {
    #[cfg(feature = "telemetry")]
    let authorization = &payment_payload.payload.permit_2_authorization;

    let PreparedUptoPermit2 {
        payer,
        eip712_hash: _,
        structured_signature,
        permit_transfer_from,
        witness,
    } = PreparedUptoPermit2::try_new(chain_reference, payment_payload)?;

    let permit2612 = x402UptoPermit2Proxy::EIP2612Permit::from(info);

    let upto_permit2_proxy = X402UptoPermit2Proxy::new(UPTO_PERMIT2_PROXY_ADDRESS, provider);
    let facilitator_address = witness.facilitator;

    let sig_bytes = Bytes::from(structured_signature);

    // Simulate with the full authorized amount (worst case)
    let max_amount = payment_payload
        .payload
        .permit_2_authorization
        .permitted
        .amount;
    let settle_call = upto_permit2_proxy
        .settleWithPermit(
            permit2612,
            permit_transfer_from,
            max_amount,
            payer,
            witness,
            sig_bytes,
        )
        .from(facilitator_address);
    let settle_call_fut = settle_call.call().into_future();
    #[cfg(feature = "telemetry")]
    settle_call_fut
        .instrument(tracing::info_span!(
            "call_settle_with_permit_upto_permit2_simulate",
            from = %payer,
            to = %authorization.witness.to,
            value = %authorization.permitted.amount,
            valid_after = %authorization.witness.valid_after,
            valid_before = %authorization.deadline,
            nonce = %authorization.nonce,
            token_contract = %authorization.permitted.token,
            otel.kind = "client",
        ))
        .await?;
    #[cfg(not(feature = "telemetry"))]
    settle_call_fut.await?;
    Ok(())
}

/// Execute `settleWithPermit` on-chain for upto payment settlement.
///
/// Uses `actual_amount` (which may be less than the authorized maximum).
#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn settle_upto_permit2_with_eip2612<P, E>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    info: &Eip2612GasSponsoringInfo,
    actual_amount: U256,
) -> Result<TxHash, Eip155ExactError>
where
    P: Eip155MetaTransactionProvider<Error = E> + ChainProviderOps,
    Eip155ExactError: From<E>,
{
    let PreparedUptoPermit2 {
        payer,
        eip712_hash: _,
        structured_signature,
        permit_transfer_from,
        witness,
    } = PreparedUptoPermit2::try_new(provider.chain(), payment_payload)?;

    let permit2612 = x402UptoPermit2Proxy::EIP2612Permit::from(info);

    let build_call = move |sig_bytes: Bytes| {
        let inner = provider.inner();
        let upto_permit2_proxy = X402UptoPermit2Proxy::new(UPTO_PERMIT2_PROXY_ADDRESS, inner);
        let facilitator_address = witness.facilitator;
        let call = upto_permit2_proxy.settleWithPermit(
            permit2612,
            permit_transfer_from,
            actual_amount,
            payer,
            witness,
            sig_bytes,
        );
        MetaTransaction::new(call.target(), call.calldata().clone()).with_from(facilitator_address)
    };

    execute_permit2_settlement(provider, payer, structured_signature, build_call).await
}
