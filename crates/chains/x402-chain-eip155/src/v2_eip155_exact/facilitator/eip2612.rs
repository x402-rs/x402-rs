//! EIP-2612 gas sponsoring extension facilitator logic.
//!
//! When a client includes the `eip2612GasSponsoring` extension in its payment payload,
//! the facilitator:
//! 1. Verifies the EIP-2612 signature spender is the canonical Permit2 address.
//! 2. Simulates (verify) or executes (settle) `x402Permit2Proxy.settleWithPermit`
//!    which atomically calls `IERC20Permit.permit` then Permit2 `permitTransferFrom`.

use alloy_primitives::{Address, Bytes, FixedBytes, TxHash, U256};
use alloy_provider::bindings::IMulticall3;
use alloy_provider::{MULTICALL3_ADDRESS, MulticallItem, Provider};
use alloy_rpc_types_eth::TransactionReceipt;
use alloy_sol_types::SolCall;
use serde::{Deserialize, Serialize};
use x402_types::chain::ChainProviderOps;
use x402_types::proto::PaymentVerificationError;
use x402_types::timestamp::UnixTimestamp;
use alloy_sol_types::{SolStruct, eip712_domain};

#[cfg(feature = "telemetry")]
use tracing::Instrument;
#[cfg(feature = "telemetry")]
use tracing::instrument;

use crate::chain::ChecksummedAddress;
use crate::chain::permit2::PERMIT2_ADDRESS;
use crate::chain::{Eip155MetaTransactionProvider, MetaTransaction};
use crate::v1_eip155_exact::{Eip155ExactError, is_contract_deployed, tx_hash_from_receipt};
use crate::v2_eip155_exact::types::{
    ISignatureTransfer, Permit2PaymentPayload, X402ExactPermit2Proxy, x402ExactPermit2Proxy,
};
use crate::chain::permit2::EXACT_PERMIT2_PROXY_ADDRESS;
use crate::v1_eip155_exact::StructuredSignature;
use crate::v2_eip155_exact::types::PermitWitnessTransferFrom;

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
    pub signature: Bytes,
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
pub struct PaymentPayloadExtensions { // FIXME THis is not used realy
    /// Optional EIP-2612 gas-sponsoring permit data.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub eip2612_gas_sponsoring: Option<Eip2612GasSponsoring>,
}

/// Extract [`Eip2612GasSponsoringInfo`] from the raw `extensions` value of a payment payload.
///
/// Returns `None` if the field is absent.
/// Returns an error if the field is present but malformed.
pub fn extract_eip2612_info( // FIXME This whole "extraction thing" feels backward
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

/// Split a 65-byte `r ++ s ++ v` signature into its components.
fn split_signature(sig: &Bytes) -> Result<(FixedBytes<32>, FixedBytes<32>, u8), Eip155ExactError> {
    if sig.len() != 65 {
        return Err(PaymentVerificationError::InvalidSignature(format!(
            "EIP-2612 signature must be 65 bytes, got {}",
            sig.len()
        ))
        .into());
    }
    let r: FixedBytes<32> = FixedBytes::from_slice(&sig[0..32]);
    let s: FixedBytes<32> = FixedBytes::from_slice(&sig[32..64]);
    let v: u8 = sig[64];
    Ok((r, s, v))
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
    use crate::chain::permit2::EXACT_PERMIT2_PROXY_ADDRESS;
    use crate::v1_eip155_exact::StructuredSignature;
    use crate::v2_eip155_exact::types::PermitWitnessTransferFrom;
    use alloy_sol_types::{SolStruct, eip712_domain};

    let authorization = &payment_payload.payload.permit_2_authorization;
    let payer: Address = authorization.from.into();

    let domain = eip712_domain! {
        name: "Permit2",
        chain_id: chain_reference.inner(),
        verifying_contract: PERMIT2_ADDRESS,
    };
    let permit_witness_transfer_from = PermitWitnessTransferFrom {
        permitted: ISignatureTransfer::TokenPermissions {
            token: authorization.permitted.token.into(),
            amount: authorization.permitted.amount,
        },
        spender: EXACT_PERMIT2_PROXY_ADDRESS,
        nonce: authorization.nonce,
        deadline: U256::from(authorization.deadline.as_secs()),
        witness: x402ExactPermit2Proxy::Witness {
            to: authorization.witness.to.into(),
            validAfter: U256::from(authorization.witness.valid_after.as_secs()),
        },
    };
    let eip712_hash = permit_witness_transfer_from.eip712_signing_hash(&domain);
    let structured_signature = StructuredSignature::try_from_bytes(
        payment_payload.payload.signature.clone(),
        payer,
        &eip712_hash,
    )?;

    let (r, s, v) = split_signature(&info.signature)?;
    let permit2612 = x402ExactPermit2Proxy::EIP2612Permit {
        value: info.amount,
        deadline: U256::from(info.deadline.as_secs()),
        r,
        s,
        v,
    };

    let exact_permit2_proxy = X402ExactPermit2Proxy::new(EXACT_PERMIT2_PROXY_ADDRESS, provider);
    let permit_transfer_from = ISignatureTransfer::PermitTransferFrom {
        permitted: permit_witness_transfer_from.permitted,
        nonce: permit_witness_transfer_from.nonce,
        deadline: permit_witness_transfer_from.deadline,
    };
    let witness = permit_witness_transfer_from.witness;

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
    let authorization = &payment_payload.payload.permit_2_authorization;
    let payer: Address = authorization.from.into();

    let domain = eip712_domain! {
        name: "Permit2",
        chain_id: provider.chain().inner(),
        verifying_contract: PERMIT2_ADDRESS,
    };
    let permit_witness_transfer_from = PermitWitnessTransferFrom {
        permitted: ISignatureTransfer::TokenPermissions {
            token: authorization.permitted.token.into(),
            amount: authorization.permitted.amount,
        },
        spender: EXACT_PERMIT2_PROXY_ADDRESS,
        nonce: authorization.nonce,
        deadline: U256::from(authorization.deadline.as_secs()),
        witness: x402ExactPermit2Proxy::Witness {
            to: authorization.witness.to.into(),
            validAfter: U256::from(authorization.witness.valid_after.as_secs()),
        },
    };
    let eip712_hash = permit_witness_transfer_from.eip712_signing_hash(&domain);
    let structured_signature = StructuredSignature::try_from_bytes(
        payment_payload.payload.signature.clone(),
        payer,
        &eip712_hash,
    )?;

    let (r, s, v) = split_signature(&info.signature)?;
    let permit2612 = x402ExactPermit2Proxy::EIP2612Permit {
        value: info.amount,
        deadline: U256::from(info.deadline.as_secs()),
        r,
        s,
        v,
    };

    let exact_permit2_proxy =
        X402ExactPermit2Proxy::new(EXACT_PERMIT2_PROXY_ADDRESS, provider.inner());
    let permit_transfer_from = ISignatureTransfer::PermitTransferFrom {
        permitted: permit_witness_transfer_from.permitted,
        nonce: permit_witness_transfer_from.nonce,
        deadline: permit_witness_transfer_from.deadline,
    };
    let witness = permit_witness_transfer_from.witness;

    let receipt: TransactionReceipt = match structured_signature {
        StructuredSignature::EIP6492 {
            factory,
            factory_calldata,
            inner,
            original: _,
        } => {
            let is_contract_deployed = is_contract_deployed(provider.inner(), &payer).await?;
            let settle_call = exact_permit2_proxy.settleWithPermit(
                permit2612,
                permit_transfer_from,
                payer,
                witness,
                inner.clone(),
            );
            if is_contract_deployed {
                let tx_fut = Eip155MetaTransactionProvider::send_transaction(
                    provider,
                    MetaTransaction {
                        to: settle_call.target(),
                        calldata: settle_call.calldata().clone(),
                        confirmations: 1,
                    },
                );
                #[cfg(feature = "telemetry")]
                let receipt = tx_fut
                    .instrument(
                        tracing::info_span!("call_exact_permit2_proxy_settle_with_permit.EIP6492.deployed",
                            from = %payer,
                            to = %authorization.witness.to,
                            value = %authorization.permitted.amount,
                            valid_after = %authorization.witness.valid_after,
                            valid_before = %authorization.deadline,
                            nonce = %authorization.nonce,
                            token_contract = %authorization.permitted.token,
                            signature = %inner,
                            sig_kind="EIP6492.deployed",
                            otel.kind = "client",
                        ),
                    )
                    .await?;
                #[cfg(not(feature = "telemetry"))]
                let receipt = tx_fut.await?;
                receipt
            } else {
                let deployment_call = IMulticall3::Call3 {
                    allowFailure: true,
                    target: factory,
                    callData: factory_calldata,
                };
                let transfer_call = IMulticall3::Call3 {
                    allowFailure: false,
                    target: settle_call.target(),
                    callData: settle_call.calldata().clone(),
                };
                let aggregate_call = IMulticall3::aggregate3Call {
                    calls: vec![deployment_call, transfer_call],
                };
                let tx_fut = Eip155MetaTransactionProvider::send_transaction(
                    provider,
                    MetaTransaction {
                        to: MULTICALL3_ADDRESS,
                        calldata: aggregate_call.abi_encode().into(),
                        confirmations: 1,
                    },
                );
                #[cfg(feature = "telemetry")]
                let receipt = tx_fut
                    .instrument(tracing::info_span!("call_exact_permit2_proxy_settle_with_permit.EIP6492.counterfactual",
                        from = %payer,
                        to = %authorization.witness.to,
                        value = %authorization.permitted.amount,
                        valid_after = %authorization.witness.valid_after,
                        valid_before = %authorization.deadline,
                        nonce = %authorization.nonce,
                        token_contract = %authorization.permitted.token,
                        signature = %inner,
                        sig_kind="EIP6492.counterfactual",
                        otel.kind = "client",
                    ))
                    .await?;
                #[cfg(not(feature = "telemetry"))]
                let receipt = tx_fut.await?;
                receipt
            }
        }
        StructuredSignature::EOA(signature) => {
            let settle_call = exact_permit2_proxy.settleWithPermit(
                permit2612,
                permit_transfer_from,
                payer,
                witness,
                signature.as_bytes().into(),
            );
            let tx_fut = Eip155MetaTransactionProvider::send_transaction(
                provider,
                MetaTransaction {
                    to: settle_call.target(),
                    calldata: settle_call.calldata().clone(),
                    confirmations: 1,
                },
            );
            #[cfg(feature = "telemetry")]
            let receipt = tx_fut
                .instrument(
                    tracing::info_span!("call_exact_permit2_proxy_settle_with_permit.EOA",
                        from = %payer,
                        to = %authorization.witness.to,
                        value = %authorization.permitted.amount,
                        valid_after = %authorization.witness.valid_after,
                        valid_before = %authorization.deadline,
                        nonce = %authorization.nonce,
                        token_contract = %authorization.permitted.token,
                        signature = %signature,
                        sig_kind="EOA",
                        otel.kind = "client",
                    ),
                )
                .await?;
            #[cfg(not(feature = "telemetry"))]
            let receipt = tx_fut.await?;
            receipt
        }
        StructuredSignature::EIP1271(signature) => {
            let settle_call = exact_permit2_proxy.settleWithPermit(
                permit2612,
                permit_transfer_from,
                payer,
                witness,
                signature.clone(),
            );
            let tx_fut = Eip155MetaTransactionProvider::send_transaction(
                provider,
                MetaTransaction {
                    to: settle_call.target(),
                    calldata: settle_call.calldata().clone(),
                    confirmations: 1,
                },
            );
            #[cfg(feature = "telemetry")]
            let receipt = tx_fut
                .instrument(
                    tracing::info_span!("call_exact_permit2_proxy_settle_with_permit.EIP1271",
                        from = %payer,
                        to = %authorization.witness.to,
                        value = %authorization.permitted.amount,
                        valid_after = %authorization.witness.valid_after,
                        valid_before = %authorization.deadline,
                        nonce = %authorization.nonce,
                        token_contract = %authorization.permitted.token,
                        signature = %signature,
                        sig_kind="EIP1271",
                        otel.kind = "client",
                    ),
                )
                .await?;
            #[cfg(not(feature = "telemetry"))]
            let receipt = tx_fut.await?;
            receipt
        }
    };
    tx_hash_from_receipt(&receipt)
}
