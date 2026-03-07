//! EIP-2612 gas sponsoring extension facilitator logic.
//!
//! When a client includes the `eip2612GasSponsoring` extension in its payment payload,
//! the facilitator:
//! 1. Verifies the EIP-2612 signature spender is the canonical Permit2 address.
//! 2. Simulates (verify) or executes (settle) `x402Permit2Proxy.settleWithPermit`
//!    which atomically calls `IERC20Permit.permit` then Permit2 `permitTransferFrom`.

use alloy_primitives::{Bytes, TxHash, U256};
use alloy_provider::{MulticallItem, Provider};
use serde::{Deserialize, Serialize};
use x402_types::chain::ChainProviderOps;
use x402_types::proto::PaymentVerificationError;
use x402_types::timestamp::UnixTimestamp;

#[cfg(feature = "telemetry")]
use tracing::Instrument;
#[cfg(feature = "telemetry")]
use tracing::instrument;

use crate::chain::permit2::EXACT_PERMIT2_PROXY_ADDRESS;
use crate::chain::permit2::PERMIT2_ADDRESS;
use crate::chain::{ChecksummedAddress, EOASignature, EOASignatureExt};
use crate::chain::{Eip155MetaTransactionProvider, MetaTransaction};
use crate::v1_eip155_exact::Eip155ExactError;
use crate::v1_eip155_exact::StructuredSignature;
use crate::v2_eip155_exact::facilitator::permit2::execute_permit2_settlement;
use crate::v2_eip155_exact::permit2::PreparedExactPermit2;
use crate::v2_eip155_exact::types::{
    Permit2PaymentPayload, X402ExactPermit2Proxy, x402ExactPermit2Proxy,
};

/// The EIP-2612 gas sponsoring extension key as it appears in the `extensions` JSON object.
pub const EXTENSION_KEY: &str = "eip2612GasSponsoring";

/// Extension info provided by the client inside the `eip2612GasSponsoring` extension.
///
/// This is the EIP-2612 permit data the client has signed to approve the canonical
/// Permit2 contract to spend tokens on its behalf.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Eip2612GasSponsoringInfo {
    /// The address of the token owner (payer).
    pub from: ChecksummedAddress,
    /// ERC-20 token contract address.
    pub asset: ChecksummedAddress,
    /// The spender that was approved (MUST be the canonical Permit2 address).
    pub spender: ChecksummedAddress,
    /// The amount approved via `permit` (typically `MaxUint256`).
    #[serde(with = "crate::decimal_u256")]
    pub amount: U256,
    /// The EIP-2612 nonce (not used in `settleWithPermit` call but available for
    /// signature validation purposes).
    #[serde(with = "crate::decimal_u256")]
    pub nonce: U256,
    /// The deadline for the EIP-2612 permit signature.
    pub deadline: UnixTimestamp,
    /// The 65-byte concatenated EIP-2612 signature `r ++ s ++ v` as a hex bytes string.
    pub signature: EOASignature,
    /// Extension schema version (currently `"1"`).
    pub version: String,
}

/// Wrapper that contains the extension info nested under `info`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Eip2612GasSponsoring {
    pub info: Eip2612GasSponsoringInfo,
}

/// Top-level extensions object that may appear in a payment payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PaymentPayloadExtensions {
    // FIXME THis is not used realy
    /// Optional EIP-2612 gas-sponsoring permit data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eip2612_gas_sponsoring: Option<Eip2612GasSponsoring>,
}

/// Extract [`Eip2612GasSponsoringInfo`] from the raw `extensions` value of a payment payload.
///
/// Returns `None` if the field is absent.
/// Returns an error if the field is present but malformed.
pub fn extract_eip2612_info(
    // FIXME This whole "extraction thing" feels backward
    extensions: &serde_json::Value,
) -> Result<Option<Eip2612GasSponsoringInfo>, PaymentVerificationError> {
    let Some(ext_obj) = extensions.as_object() else {
        return Err(PaymentVerificationError::InvalidFormat(
            "extensions is not an object".to_string(),
        ));
    };
    let Some(raw) = ext_obj.get(EXTENSION_KEY) else {
        return Ok(None);
    };
    let sponsoring: Eip2612GasSponsoring = serde_json::from_value(raw.clone())
        .map_err(|e| PaymentVerificationError::InvalidFormat(e.to_string()))?;
    Ok(Some(sponsoring.info))
}

/// Verify the offchain constraints of the EIP-2612 gas-sponsoring extension.
///
/// Checks:
/// - `spender` in the extension is the canonical Permit2 address
/// - `asset` matches the asset in the payment accepted requirements
/// - `from` matches the payer in the Permit2 authorization
/// - `deadline` is not expired
#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub fn assert_eip2612_offchain_valid(
    info: &Eip2612GasSponsoringInfo,
    payment_payload: &Permit2PaymentPayload,
) -> Result<(), PaymentVerificationError> {
    // spender must be the canonical Permit2
    if info.spender.0 != PERMIT2_ADDRESS {
        return Err(PaymentVerificationError::InvalidSignature(
            "eip2612GasSponsoring spender must be the canonical Permit2 address".to_string(),
        ));
    }

    // asset must match
    if info.asset != payment_payload.accepted.asset {
        return Err(PaymentVerificationError::AssetMismatch);
    }

    // from must match permit2 authorization from
    let authorization = &payment_payload.payload.permit_2_authorization;
    if info.from != authorization.from {
        return Err(PaymentVerificationError::InvalidSignature(
            "eip2612GasSponsoring 'from' does not match permit2 authorization 'from'".to_string(),
        ));
    }

    // deadline must be >= the Permit2 deadline (permit must stay valid long enough)
    if info.deadline < authorization.deadline {
        return Err(PaymentVerificationError::Expired);
    }

    Ok(())
}

/// Simulate `settleWithPermit` on-chain for payment verification.
///
/// This replaces the usual `assert_onchain_exact_permit2` simulation when the
/// `eip2612GasSponsoring` extension is present in the payment payload.
#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn assert_onchain_exact_permit2_with_eip2612<P: Provider>(
    provider: &P,
    chain_reference: &crate::chain::Eip155ChainReference,
    payment_payload: &Permit2PaymentPayload,
    info: &Eip2612GasSponsoringInfo,
) -> Result<(), Eip155ExactError> {
    #[cfg(feature = "telemetry")]
    let authorization = &payment_payload.payload.permit_2_authorization;

    let PreparedExactPermit2 {
        payer,
        eip712_hash: _,
        structured_signature,
        permit_transfer_from,
        witness,
    } = PreparedExactPermit2::try_new(chain_reference, payment_payload)?;

    let permit2612 = x402ExactPermit2Proxy::EIP2612Permit {
        value: info.amount,
        deadline: U256::from(info.deadline.as_secs()),
        r: info.signature.r_bytes(),
        s: info.signature.s_bytes(),
        v: info.signature.v_legacy(),
    };

    let exact_permit2_proxy = X402ExactPermit2Proxy::new(EXACT_PERMIT2_PROXY_ADDRESS, provider);

    let sig_bytes: Bytes = match &structured_signature {
        StructuredSignature::EIP6492 { inner, .. } => inner.clone(),
        StructuredSignature::EOA(sig) => sig.as_bytes().into(),
        StructuredSignature::EIP1271(sig) => sig.clone(),
    };

    let settle_call = exact_permit2_proxy.settleWithPermit(
        permit2612,
        permit_transfer_from,
        payer,
        witness,
        sig_bytes,
    );
    let settle_call_fut = settle_call.call().into_future();
    #[cfg(feature = "telemetry")]
    settle_call_fut
        .instrument(
            tracing::info_span!("call_settle_with_permit_exact_permit2_simulate",
                from = %payer,
                to = %authorization.witness.to,
                value = %authorization.permitted.amount,
                valid_after = %authorization.witness.valid_after,
                valid_before = %authorization.deadline,
                nonce = %authorization.nonce,
                token_contract = %authorization.permitted.token,
                otel.kind = "client",
            ),
        )
        .await?;
    #[cfg(not(feature = "telemetry"))]
    settle_call_fut.await?;
    Ok(())
}

/// Execute `settleWithPermit` on-chain for payment settlement.
///
/// This replaces the usual `settle_exact_permit2` call when the
/// `eip2612GasSponsoring` extension is present in the payment payload.
#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn settle_exact_permit2_with_eip2612<P, E>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    info: &Eip2612GasSponsoringInfo,
) -> Result<TxHash, Eip155ExactError>
where
    P: Eip155MetaTransactionProvider<Error = E> + ChainProviderOps,
    Eip155ExactError: From<E>,
{
    let PreparedExactPermit2 {
        payer,
        eip712_hash: _,
        structured_signature,
        permit_transfer_from,
        witness,
    } = PreparedExactPermit2::try_new(provider.chain(), payment_payload)?;

    let permit2612 = x402ExactPermit2Proxy::EIP2612Permit {
        value: info.amount,
        deadline: U256::from(info.deadline.as_secs()),
        r: info.signature.r_bytes(),
        s: info.signature.s_bytes(),
        v: info.signature.v_legacy(),
    };

    let build_call = move |sig_bytes: Bytes| {
        let inner = provider.inner();
        let exact_permit2_proxy = X402ExactPermit2Proxy::new(EXACT_PERMIT2_PROXY_ADDRESS, inner);
        let call = exact_permit2_proxy.settleWithPermit(
            permit2612,
            permit_transfer_from,
            payer,
            witness,
            sig_bytes,
        );
        MetaTransaction::new(call.target(), call.calldata().clone())
    };

    execute_permit2_settlement(provider, payer, structured_signature, build_call).await
}
