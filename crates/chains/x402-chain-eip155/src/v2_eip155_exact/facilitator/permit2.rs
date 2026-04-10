use alloy_primitives::{Address, Bytes, TxHash, U256};
use alloy_provider::bindings::IMulticall3;
use alloy_provider::{MULTICALL3_ADDRESS, MulticallItem, Provider};
use alloy_rpc_types_eth::TransactionReceipt;
use alloy_sol_types::{SolCall, SolStruct, eip712_domain};
use serde::Deserialize;
use std::collections::HashMap;
use x402_types::chain::ChainProviderOps;
use x402_types::timestamp::UnixTimestamp;
use x402_types::proto::{PaymentVerificationError, v2};
use x402_types::scheme::X402SchemeFacilitatorError;

#[cfg(feature = "telemetry")]
use tracing::Instrument;
#[cfg(feature = "telemetry")]
use tracing::instrument;

use crate::chain::erc20::IERC20;
use crate::chain::permit2::{EXACT_PERMIT2_PROXY_ADDRESS, PERMIT2_ADDRESS};
use crate::chain::{Eip155ChainReference, Eip155MetaTransactionProvider, MetaTransaction};
use crate::v1_eip155_exact::{
    Eip155ExactError, StructuredSignature, VALIDATOR_ADDRESS, Validator6492, assert_enough_value,
    assert_time, is_contract_deployed, tx_hash_from_receipt,
};
use crate::v2_eip155_exact::facilitator::V2Eip155ExactFacilitatorConfig;
use crate::v2_eip155_exact::eip3009::assert_requirements_match;
use crate::v2_eip155_exact::types::{
    ISignatureTransfer, Permit2PaymentPayload, Permit2PaymentRequirements,
    PermitWitnessTransferFrom, X402ExactPermit2Proxy, x402BasePermit2Proxy,
};

pub const EXTENSION_EIP2612_GAS_SPONSORING: &str = "eip2612GasSponsoring";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Eip2612GasSponsoringInfo {
    from: crate::chain::ChecksummedAddress,
    asset: crate::chain::ChecksummedAddress,
    spender: crate::chain::ChecksummedAddress,
    #[serde(with = "crate::decimal_u256")]
    amount: U256,
    #[serde(rename = "nonce")]
    #[serde(with = "crate::decimal_u256")]
    _nonce: U256,
    deadline: UnixTimestamp,
    signature: Bytes,
    version: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Eip2612GasSponsoringExtension {
    info: Eip2612GasSponsoringInfo,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum Eip2612GasSponsoringWire {
    Wrapped(Eip2612GasSponsoringExtension),
    BareInfo(Eip2612GasSponsoringInfo),
}

impl Eip2612GasSponsoringWire {
    fn into_info(self) -> Eip2612GasSponsoringInfo {
        match self {
            Self::Wrapped(extension) => extension.info,
            Self::BareInfo(info) => info,
        }
    }
}

fn extract_eip2612_gas_sponsoring_info(
    extensions: &HashMap<String, serde_json::Value>,
) -> Result<Option<Eip2612GasSponsoringInfo>, PaymentVerificationError> {
    let Some(raw) = extensions.get(EXTENSION_EIP2612_GAS_SPONSORING) else {
        return Ok(None);
    };
    let parsed: Eip2612GasSponsoringWire = serde_json::from_value(raw.clone())?;
    Ok(Some(parsed.into_info()))
}

fn split_eip2612_signature(signature: &Bytes) -> Result<(u8, [u8; 32], [u8; 32]), Eip155ExactError> {
    if signature.len() != 65 {
        return Err(PaymentVerificationError::InvalidFormat(
            "eip2612 signature must be 65 bytes".to_string(),
        )
        .into());
    }

    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&signature[..32]);
    s.copy_from_slice(&signature[32..64]);
    Ok((signature[64], r, s))
}

fn validate_eip2612_gas_sponsoring_info(
    info: &Eip2612GasSponsoringInfo,
    payment_payload: &Permit2PaymentPayload,
) -> Result<(), PaymentVerificationError> {
    let authorization = &payment_payload.payload.permit_2_authorization;
    let accepted = &payment_payload.accepted;

    if info.from != authorization.from {
        return Err(PaymentVerificationError::InvalidFormat(
            "eip2612 from does not match permit2 payer".to_string(),
        ));
    }
    if info.asset != accepted.asset {
        return Err(PaymentVerificationError::AssetMismatch);
    }
    if info.spender.0 != PERMIT2_ADDRESS {
        return Err(PaymentVerificationError::InvalidFormat(
            "eip2612 spender must be canonical Permit2".to_string(),
        ));
    }
    if info.amount != authorization.permitted.amount {
        return Err(PaymentVerificationError::InvalidPaymentAmount);
    }
    if info.deadline < UnixTimestamp::now() + 6 {
        return Err(PaymentVerificationError::Expired);
    }
    if info.version.trim().is_empty() {
        return Err(PaymentVerificationError::InvalidFormat(
            "eip2612 version is required".to_string(),
        ));
    }
    Ok(())
}

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn verify_permit2_payment<P: Eip155MetaTransactionProvider + ChainProviderOps>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
    config: &V2Eip155ExactFacilitatorConfig,
) -> Result<v2::VerifyResponse, Eip155ExactError> {
    // 1. Verify offchain constraints
    assert_offchain_valid(payment_payload, payment_requirements)?;

    // 2. Verify onchain constraints
    let authorization = &payment_payload.payload.permit_2_authorization;
    let payer: Address = authorization.from.into();
    let eip2612 = if config.supports_extension(EXTENSION_EIP2612_GAS_SPONSORING) {
        extract_eip2612_gas_sponsoring_info(&payment_payload.extensions)?
    } else {
        None
    };
    assert_onchain_exact_permit2(provider.inner(), provider.chain(), payment_payload, eip2612.as_ref())
        .await?;

    Ok(v2::VerifyResponse::valid(payer.to_string()))
}

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn settle_permit2_payment<P, E>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
    config: &V2Eip155ExactFacilitatorConfig,
) -> Result<v2::SettleResponse, X402SchemeFacilitatorError>
where
    P: Eip155MetaTransactionProvider<Error = E> + ChainProviderOps,
    Eip155ExactError: From<E>,
{
    // 1. Verify offchain constraints
    assert_offchain_valid(payment_payload, payment_requirements)?;

    // 2. Try settle
    let eip2612 = if config.supports_extension(EXTENSION_EIP2612_GAS_SPONSORING) {
        extract_eip2612_gas_sponsoring_info(&payment_payload.extensions)?
    } else {
        None
    };
    let tx_hash = settle_exact_permit2(provider, payment_payload, eip2612.as_ref()).await?;
    let authorization = &payment_payload.payload.permit_2_authorization;
    let payer = authorization.from;
    let network = &payment_payload.accepted.network;

    Ok(v2::SettleResponse::Success {
        payer: payer.to_string(),
        transaction: tx_hash.to_string(),
        network: network.to_string(),
    })
}

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub fn assert_offchain_valid(
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
) -> Result<(), PaymentVerificationError> {
    let payload = &payment_payload.payload;
    let accepted = &payment_payload.accepted;
    assert_requirements_match(accepted, payment_requirements)?;

    // Spender must be the x402ExactPermit2Proxy contract address
    let authorization = &payload.permit_2_authorization;
    if authorization.spender.0 != EXACT_PERMIT2_PROXY_ADDRESS {
        return Err(PaymentVerificationError::RecipientMismatch);
    }

    // Correct recipient
    let witness = &authorization.witness;
    if witness.to != accepted.pay_to {
        return Err(PaymentVerificationError::RecipientMismatch);
    }

    // Time validity
    let valid_after = witness.valid_after;
    let valid_before = authorization.deadline;
    assert_time(valid_after, valid_before)?;

    // Sufficient amount
    let amount_required = &accepted.amount;
    assert_enough_value(&authorization.permitted.amount, amount_required)?;

    // Same token
    if authorization.permitted.token != accepted.asset {
        return Err(PaymentVerificationError::AssetMismatch);
    }
    Ok(())
}

pub async fn assert_onchain_allowance<P: Provider>(
    token_contract: &IERC20::IERC20Instance<P>,
    payer: Address,
    required_amount: U256,
) -> Result<(), Eip155ExactError> {
    let allowance_call = token_contract.allowance(payer, PERMIT2_ADDRESS);
    let allowance_fut = allowance_call.call().into_future();
    #[cfg(feature = "telemetry")]
    let allowance = allowance_fut
        .instrument(tracing::info_span!(
            "fetch_permit2_allowance",
            token_contract = %token_contract.address(),
            sender = %payer,
            otel.kind = "client"
        ))
        .await?;
    #[cfg(not(feature = "telemetry"))]
    let allowance = allowance_fut.await?;
    if allowance < required_amount {
        Err(PaymentVerificationError::InsufficientAllowance.into())
    } else {
        Ok(())
    }
}

pub async fn assert_onchain_balance<P: Provider>(
    token_contract: &IERC20::IERC20Instance<P>,
    payer: Address,
    required_amount: U256,
) -> Result<(), Eip155ExactError> {
    let balance_call = token_contract.balanceOf(payer);
    let balance_fut = balance_call.call().into_future();
    #[cfg(feature = "telemetry")]
    let balance = balance_fut
        .instrument(tracing::info_span!(
            "fetch_balance",
            token_contract = %token_contract.address(),
            sender = %payer,
            otel.kind = "client"
        ))
        .await?;
    #[cfg(not(feature = "telemetry"))]
    let balance = balance_fut.await?;
    if balance < required_amount {
        return Err(PaymentVerificationError::InsufficientFunds.into());
    }
    Ok(())
}

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
async fn assert_onchain_exact_permit2<P: Provider>(
    provider: &P,
    chain_reference: &Eip155ChainReference,
    payment_payload: &Permit2PaymentPayload,
    eip2612: Option<&Eip2612GasSponsoringInfo>,
) -> Result<(), Eip155ExactError> {
    let authorization = &payment_payload.payload.permit_2_authorization;
    let payer = authorization.from.0;
    let required_amount = payment_payload.accepted.amount;
    let asset_address = payment_payload.accepted.asset.0;

    let token_contract = IERC20::new(asset_address, provider);

    // User balance is enough
    assert_onchain_balance(&token_contract, payer, required_amount).await?;

    let has_allowance = assert_onchain_allowance(&token_contract, payer, required_amount)
        .await
        .is_ok();
    let should_use_eip2612 = !has_allowance && eip2612.is_some();
    if !has_allowance && !should_use_eip2612 {
        return Err(PaymentVerificationError::InsufficientAllowance.into());
    }
    if let Some(info) = eip2612 {
        validate_eip2612_gas_sponsoring_info(info, payment_payload)?;
    }

    // ... and below is a check if we can do the settle

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
        witness: x402BasePermit2Proxy::Witness {
            to: authorization.witness.to.into(),
            validAfter: U256::from(authorization.witness.valid_after.as_secs()),
            extra: authorization.witness.extra.clone(),
        },
    };
    let eip712_hash = permit_witness_transfer_from.eip712_signing_hash(&domain);
    let structured_signature = StructuredSignature::try_from_bytes(
        payment_payload.payload.signature.clone(),
        payer,
        &eip712_hash,
    )?;

    let exact_permit2_proxy = X402ExactPermit2Proxy::new(EXACT_PERMIT2_PROXY_ADDRESS, provider);
    let eip2612_permit = if let Some(info) = eip2612 {
        let (v, r, s) = split_eip2612_signature(&info.signature)?;
        Some(x402BasePermit2Proxy::EIP2612Permit {
            value: info.amount,
            deadline: U256::from(info.deadline.as_secs()),
            r: r.into(),
            s: s.into(),
            v,
        })
    } else {
        None
    };
    match structured_signature {
        StructuredSignature::EIP6492 {
            factory: _,
            factory_calldata: _,
            inner,
            original,
        } => {
            let validator6492 = Validator6492::new(VALIDATOR_ADDRESS, provider);
            let is_valid_signature_call =
                validator6492.isValidSigWithSideEffects(payer, eip712_hash, original);
            let permit_transfer_from = ISignatureTransfer::PermitTransferFrom {
                permitted: permit_witness_transfer_from.permitted,
                nonce: permit_witness_transfer_from.nonce,
                deadline: permit_witness_transfer_from.deadline,
            };
            let witness = permit_witness_transfer_from.witness;
            if let Some(permit2612) = eip2612_permit.clone() {
                let settle_call = exact_permit2_proxy.settleWithPermit(
                    permit2612,
                    permit_transfer_from,
                    payer,
                    witness,
                    inner,
                );
                let aggregate3 = provider
                    .multicall()
                    .add(is_valid_signature_call)
                    .add(settle_call);
                let aggregate3_call = aggregate3.aggregate3();
                #[cfg(feature = "telemetry")]
                let (is_valid_signature_result, transfer_result) = aggregate3_call
                    .instrument(tracing::info_span!("multi_call_settle_exact_permit2",
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
                let (is_valid_signature_result, transfer_result) = aggregate3_call.await?;
                let is_valid_signature_result = is_valid_signature_result
                    .map_err(|e| PaymentVerificationError::InvalidSignature(e.to_string()))?;
                if !is_valid_signature_result {
                    return Err(PaymentVerificationError::InvalidSignature(
                        "Chain reported signature to be invalid".to_string(),
                    )
                    .into());
                }
                transfer_result
                    .map_err(|e| PaymentVerificationError::TransactionSimulation(e.to_string()))?;
            } else {
                let settle_call = exact_permit2_proxy.settle(
                    permit_transfer_from,
                    payer,
                    witness,
                    inner,
                );
                let aggregate3 = provider
                    .multicall()
                    .add(is_valid_signature_call)
                    .add(settle_call);
                let aggregate3_call = aggregate3.aggregate3();
                #[cfg(feature = "telemetry")]
                let (is_valid_signature_result, transfer_result) = aggregate3_call
                    .instrument(tracing::info_span!("multi_call_settle_exact_permit2",
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
                let (is_valid_signature_result, transfer_result) = aggregate3_call.await?;
                let is_valid_signature_result = is_valid_signature_result
                    .map_err(|e| PaymentVerificationError::InvalidSignature(e.to_string()))?;
                if !is_valid_signature_result {
                    return Err(PaymentVerificationError::InvalidSignature(
                        "Chain reported signature to be invalid".to_string(),
                    )
                    .into());
                }
                transfer_result
                    .map_err(|e| PaymentVerificationError::TransactionSimulation(e.to_string()))?;
            }
            Ok(())
        }
        StructuredSignature::EOA(signature) => {
            let permit_transfer_from = ISignatureTransfer::PermitTransferFrom {
                permitted: permit_witness_transfer_from.permitted,
                nonce: permit_witness_transfer_from.nonce,
                deadline: permit_witness_transfer_from.deadline,
            };
            let witness = permit_witness_transfer_from.witness;
            if let Some(permit2612) = eip2612_permit.clone() {
                let settle_call = exact_permit2_proxy.settleWithPermit(
                    permit2612,
                    permit_transfer_from,
                    payer,
                    witness,
                    signature.as_bytes().into(),
                );
                let settle_call_fut = settle_call.call().into_future();
                #[cfg(feature = "telemetry")]
                settle_call_fut
                    .instrument(tracing::info_span!("call_settle_exact_permit2",
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
            } else {
                let settle_call = exact_permit2_proxy.settle(
                    permit_transfer_from,
                    payer,
                    witness,
                    signature.as_bytes().into(),
                );
                let settle_call_fut = settle_call.call().into_future();
                #[cfg(feature = "telemetry")]
                settle_call_fut
                    .instrument(tracing::info_span!("call_settle_exact_permit2",
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
            }
            Ok(())
        }
        StructuredSignature::EIP1271(signature) => {
            let permit_transfer_from = ISignatureTransfer::PermitTransferFrom {
                permitted: permit_witness_transfer_from.permitted,
                nonce: permit_witness_transfer_from.nonce,
                deadline: permit_witness_transfer_from.deadline,
            };
            let witness = permit_witness_transfer_from.witness;
            if let Some(permit2612) = eip2612_permit {
                let settle_call = exact_permit2_proxy.settleWithPermit(
                    permit2612,
                    permit_transfer_from,
                    payer,
                    witness,
                    signature,
                );
                let settle_call_fut = settle_call.call().into_future();
                #[cfg(feature = "telemetry")]
                settle_call_fut
                    .instrument(tracing::info_span!("call_settle_exact_permit2",
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
            } else {
                let settle_call =
                    exact_permit2_proxy.settle(permit_transfer_from, payer, witness, signature);
                let settle_call_fut = settle_call.call().into_future();
                #[cfg(feature = "telemetry")]
                settle_call_fut
                    .instrument(tracing::info_span!("call_settle_exact_permit2",
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
            }
            Ok(())
        }
    }
}

async fn settle_exact_permit2<P, E>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    eip2612: Option<&Eip2612GasSponsoringInfo>,
) -> Result<TxHash, Eip155ExactError>
where
    P: Eip155MetaTransactionProvider<Error = E> + ChainProviderOps,
    Eip155ExactError: From<E>,
{
    let authorization = &payment_payload.payload.permit_2_authorization;
    let payer = authorization.from.0;
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
        witness: x402BasePermit2Proxy::Witness {
            to: authorization.witness.to.into(),
            validAfter: U256::from(authorization.witness.valid_after.as_secs()),
            extra: authorization.witness.extra.clone(),
        },
    };
    let eip712_hash = permit_witness_transfer_from.eip712_signing_hash(&domain);
    let structured_signature = StructuredSignature::try_from_bytes(
        payment_payload.payload.signature.clone(),
        payer,
        &eip712_hash,
    )?;

    let exact_permit2_proxy =
        X402ExactPermit2Proxy::new(EXACT_PERMIT2_PROXY_ADDRESS, provider.inner());
    let eip2612_permit = if let Some(info) = eip2612 {
        let (v, r, s) = split_eip2612_signature(&info.signature)?;
        Some(x402BasePermit2Proxy::EIP2612Permit {
            value: info.amount,
            deadline: U256::from(info.deadline.as_secs()),
            r: r.into(),
            s: s.into(),
            v,
        })
    } else {
        None
    };
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
            let (settle_target, settle_calldata) = if let Some(permit2612) = eip2612_permit.clone() {
                let settle_call = exact_permit2_proxy.settleWithPermit(
                    permit2612,
                    permit_transfer_from,
                    payer,
                    witness,
                    inner.clone(),
                );
                (settle_call.target(), settle_call.calldata().clone())
            } else {
                let settle_call =
                    exact_permit2_proxy.settle(permit_transfer_from, payer, witness, inner.clone());
                (settle_call.target(), settle_call.calldata().clone())
            };
            if is_contract_deployed {
                let tx_fut = Eip155MetaTransactionProvider::send_transaction(
                    provider,
                    MetaTransaction {
                        to: settle_target,
                        calldata: settle_calldata.clone(),
                        confirmations: 1,
                    },
                );
                #[cfg(feature = "telemetry")]
                let receipt = tx_fut
                    .instrument(
                        tracing::info_span!("call_exact_permit2_proxy_settle.EIP6492.deployed",
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
                // deploy the smart wallet, and settle with inner signature
                let deployment_call = IMulticall3::Call3 {
                    allowFailure: true,
                    target: factory,
                    callData: factory_calldata,
                };
                let transfer_with_authorization_call = IMulticall3::Call3 {
                    allowFailure: false,
                    target: settle_target,
                    callData: settle_calldata.clone(),
                };
                let aggregate_call = IMulticall3::aggregate3Call {
                    calls: vec![deployment_call, transfer_with_authorization_call],
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
                    .instrument(tracing::info_span!("call_exact_permit2_proxy_settle.EIP6492.counterfactual",
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
            let tx_fut = if let Some(permit2612) = eip2612_permit.clone() {
                let settle_call = exact_permit2_proxy.settleWithPermit(
                    permit2612,
                    permit_transfer_from,
                    payer,
                    witness,
                    signature.as_bytes().into(),
                );
                Eip155MetaTransactionProvider::send_transaction(
                    provider,
                    MetaTransaction {
                        to: settle_call.target(),
                        calldata: settle_call.calldata().clone(),
                        confirmations: 1,
                    },
                )
            } else {
                let settle_call = exact_permit2_proxy.settle(
                    permit_transfer_from,
                    payer,
                    witness,
                    signature.as_bytes().into(),
                );
                Eip155MetaTransactionProvider::send_transaction(
                    provider,
                    MetaTransaction {
                        to: settle_call.target(),
                        calldata: settle_call.calldata().clone(),
                        confirmations: 1,
                    },
                )
            };
            #[cfg(feature = "telemetry")]
            let receipt = tx_fut
                .instrument(tracing::info_span!("call_exact_permit2_proxy_settle.EOA",
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
                ))
                .await?;
            #[cfg(not(feature = "telemetry"))]
            let receipt = tx_fut.await?;
            receipt
        }
        StructuredSignature::EIP1271(signature) => {
            let tx_fut = if let Some(permit2612) = eip2612_permit {
                let settle_call = exact_permit2_proxy.settleWithPermit(
                    permit2612,
                    permit_transfer_from,
                    payer,
                    witness,
                    signature.clone(),
                );
                Eip155MetaTransactionProvider::send_transaction(
                    provider,
                    MetaTransaction {
                        to: settle_call.target(),
                        calldata: settle_call.calldata().clone(),
                        confirmations: 1,
                    },
                )
            } else {
                let settle_call =
                    exact_permit2_proxy.settle(permit_transfer_from, payer, witness, signature.clone());
                Eip155MetaTransactionProvider::send_transaction(
                    provider,
                    MetaTransaction {
                        to: settle_call.target(),
                        calldata: settle_call.calldata().clone(),
                        confirmations: 1,
                    },
                )
            };
            #[cfg(feature = "telemetry")]
            let receipt = tx_fut
                .instrument(
                    tracing::info_span!("call_exact_permit2_proxy_settle.EIP1271",
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

#[cfg(test)]
mod tests {
    use super::{
        EXTENSION_EIP2612_GAS_SPONSORING, Eip2612GasSponsoringInfo,
        extract_eip2612_gas_sponsoring_info, validate_eip2612_gas_sponsoring_info,
    };
    use alloy_primitives::{Bytes, U256};
    use std::collections::HashMap;
    use x402_types::proto::PaymentVerificationError;
    use x402_types::timestamp::UnixTimestamp;

    use crate::chain::permit2::PERMIT2_ADDRESS;
    use crate::v2_eip155_exact::types::Permit2PaymentPayload;

    fn sample_payment_payload() -> Permit2PaymentPayload {
        serde_json::from_value(serde_json::json!({
            "x402Version": 2,
            "accepted": {
                "scheme": "exact",
                "network": "eip155:1",
                "amount": "1000000000000000000",
                "payTo": "0x1111111111111111111111111111111111111111",
                "maxTimeoutSeconds": 60,
                "asset": "0x2222222222222222222222222222222222222222",
                "extra": { "assetTransferMethod": "permit2" }
            },
            "payload": {
                "permit2Authorization": {
                    "deadline": "4294967295",
                    "from": "0x3333333333333333333333333333333333333333",
                    "nonce": "1",
                    "permitted": {
                        "amount": "1000000000000000000",
                        "token": "0x2222222222222222222222222222222222222222"
                    },
                    "spender": "0x4020615294c913F045dc10f0a5cdEbd86c280001",
                    "witness": {
                        "extra": "0x",
                        "to": "0x1111111111111111111111111111111111111111",
                        "validAfter": "0"
                    }
                },
                "signature": "0x111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111b"
            },
            "extensions": {}
        }))
        .expect("valid permit2 payload")
    }

    #[test]
    fn extracts_wrapped_eip2612_extension_info() {
        let mut extensions = HashMap::new();
        extensions.insert(
            EXTENSION_EIP2612_GAS_SPONSORING.to_string(),
            serde_json::json!({
                "info": {
                    "from": "0x3333333333333333333333333333333333333333",
                    "asset": "0x2222222222222222222222222222222222222222",
                    "spender": format!("{PERMIT2_ADDRESS:#x}"),
                    "amount": "1000000000000000000",
                    "nonce": "0",
                    "deadline": "4294967295",
                    "signature": "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa1b",
                    "version": "1"
                }
            }),
        );

        let info = extract_eip2612_gas_sponsoring_info(&extensions)
            .expect("valid extension")
            .expect("extension present");
        assert_eq!(info.amount, U256::from(1_000_000_000_000_000_000u128));
        assert_eq!(info.spender.0, PERMIT2_ADDRESS);
    }

    #[test]
    fn rejects_mismatched_eip2612_amount() {
        let payment_payload = sample_payment_payload();
        let info = Eip2612GasSponsoringInfo {
            from: payment_payload.payload.permit_2_authorization.from,
            asset: payment_payload.accepted.asset,
            spender: PERMIT2_ADDRESS.into(),
            amount: U256::from(2u64),
            _nonce: U256::ZERO,
            deadline: UnixTimestamp::from_secs(4_294_967_295),
            signature: Bytes::from_static(&[0x11; 65]),
            version: "1".to_string(),
        };

        let err = validate_eip2612_gas_sponsoring_info(&info, &payment_payload)
            .expect_err("amount mismatch should fail");
        assert!(matches!(err, PaymentVerificationError::InvalidPaymentAmount));
    }
}
