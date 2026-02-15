use alloy_primitives::{Address, TxHash, U256};
use alloy_provider::bindings::IMulticall3;
use alloy_provider::{MULTICALL3_ADDRESS, MulticallItem, Provider};
use alloy_rpc_types_eth::TransactionReceipt;
use alloy_sol_types::{SolCall, SolStruct, eip712_domain, sol};
use x402_types::chain::ChainProviderOps;
use x402_types::proto::{PaymentVerificationError, v2};
use x402_types::scheme::X402SchemeFacilitatorError;

#[cfg(feature = "telemetry")]
use tracing::Instrument;
#[cfg(feature = "telemetry")]
use tracing::instrument;

use crate::chain::{Eip155ChainReference, Eip155MetaTransactionProvider, MetaTransaction};
use crate::v1_eip155_exact::{
    Eip155ExactError, StructuredSignature, VALIDATOR_ADDRESS, Validator6492, assert_time,
    is_contract_deployed, tx_hash_from_receipt,
};
use crate::v2_eip155_upto::types;
use crate::v2_eip155_upto::types::{
    ISignatureTransfer, Permit2PaymentPayload, Permit2PaymentRequirements,
    PermitWitnessTransferFrom, X402UptoPermit2Proxy, x402BasePermit2Proxy,
};

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[derive(Debug)]
    #[sol(rpc)]
    IERC20,
    "abi/IERC20.json"
);

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn verify_permit2_payment<P: Eip155MetaTransactionProvider + ChainProviderOps>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
) -> Result<v2::VerifyResponse, Eip155ExactError> {
    // 1. Verify offchain constraints
    assert_offchain_valid(payment_payload, payment_requirements)?;

    // 2. Verify onchain constraints
    let authorization = &payment_payload.payload.permit_2_authorization;
    let payer: Address = authorization.from.into();
    assert_onchain_upto_permit2(provider.inner(), provider.chain(), payment_payload).await?;

    Ok(v2::VerifyResponse::valid(payer.to_string()))
}

/// Settle a upto permit2 payment with a specific amount.
///
/// The `settle_amount` must be less than or equal to the authorized maximum amount.
/// If `settle_amount` is `None`, the full authorized amount will be used.
#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn settle_permit2_payment<P, E>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
    settle_amount: Option<U256>,
) -> Result<v2::SettleResponse, X402SchemeFacilitatorError>
where
    P: Eip155MetaTransactionProvider<Error = E> + ChainProviderOps,
    Eip155ExactError: From<E>,
{
    // 1. Verify offchain constraints
    assert_offchain_valid(payment_payload, payment_requirements)?;

    // 2. Determine the actual settlement amount
    let authorization = &payment_payload.payload.permit_2_authorization;
    let max_amount = authorization.permitted.amount;
    let actual_amount = settle_amount.unwrap_or(max_amount);

    // 3. Validate settlement amount doesn't exceed maximum
    if actual_amount > max_amount {
        return Err(X402SchemeFacilitatorError::PaymentVerification(
            PaymentVerificationError::InvalidFormat(
                "invalid_upto_evm_payload_settlement_exceeds_amount".to_string(),
            ),
        ));
    }

    // 4. Handle zero settlement - no on-chain transaction needed
    if actual_amount.is_zero() {
        let payer = authorization.from.clone();
        let network = &payment_payload.accepted.network;
        return Ok(v2::SettleResponse::Success {
            payer: payer.to_string(),
            transaction: String::new(), // Empty transaction for $0 settlement
            network: network.to_string(),
        });
    }

    // 5. Execute settlement
    let tx_hash = settle_upto_permit2(provider, payment_payload, actual_amount).await?;
    let payer = authorization.from.clone();
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

    // Verify scheme matches
    if accepted.scheme != types::UptoScheme {
        return Err(PaymentVerificationError::UnsupportedScheme);
    }

    // Verify network matches
    if accepted.network != payment_requirements.network {
        return Err(PaymentVerificationError::ChainIdMismatch);
    }

    // Verify asset matches
    if accepted.asset != payment_requirements.asset {
        return Err(PaymentVerificationError::AssetMismatch);
    }

    // Spender must be the x402UptoPermit2Proxy contract address
    let authorization = &payload.permit_2_authorization;
    if authorization.spender.0 != types::UPTO_PERMIT2_PROXY_ADDRESS {
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

    // For upto: authorized amount must EQUAL the required amount (client authorizes exact max)
    // The server can then settle for any amount <= this max
    if authorization.permitted.amount != accepted.amount {
        return Err(PaymentVerificationError::InvalidPaymentAmount);
    }

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
    let allowance_call = token_contract.allowance(payer, types::PERMIT2_ADDRESS);
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
pub async fn assert_onchain_upto_permit2<P: Provider>(
    provider: &P,
    chain_reference: &Eip155ChainReference,
    payment_payload: &Permit2PaymentPayload,
) -> Result<(), Eip155ExactError> {
    let authorization = &payment_payload.payload.permit_2_authorization;
    let payer = authorization.from.0;
    let required_amount = payment_payload.accepted.amount;
    let asset_address = payment_payload.accepted.asset.0;

    let token_contract = IERC20::new(asset_address, provider);

    // Allowance from payer to Permit2 contract is enough
    let onchain_allowance_fut = assert_onchain_allowance(&token_contract, payer, required_amount);
    // User balance is enough
    let onchain_balance_fut = assert_onchain_balance(&token_contract, payer, required_amount);
    tokio::try_join!(onchain_allowance_fut, onchain_balance_fut)?;

    // ... and below is a check if we can do the settle
    // For upto, we simulate with the max amount (worst case)

    let domain = eip712_domain! {
        name: "Permit2",
        chain_id: chain_reference.inner(),
        verifying_contract: types::PERMIT2_ADDRESS,
    };
    let permit_witness_transfer_from = PermitWitnessTransferFrom {
        permitted: ISignatureTransfer::TokenPermissions {
            token: authorization.permitted.token.into(),
            amount: authorization.permitted.amount,
        },
        spender: types::UPTO_PERMIT2_PROXY_ADDRESS,
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

    let upto_permit2_proxy =
        X402UptoPermit2Proxy::new(types::UPTO_PERMIT2_PROXY_ADDRESS, provider);
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
            // For verification, simulate with max amount
            let settle_call =
                upto_permit2_proxy.settle(permit_transfer_from, authorization.permitted.amount, payer, witness, inner);
            let aggregate3 = provider
                .multicall()
                .add(is_valid_signature_call)
                .add(settle_call);
            let aggregate3_call = aggregate3.aggregate3();
            #[cfg(feature = "telemetry")]
            let (is_valid_signature_result, transfer_result) = aggregate3_call
                .instrument(tracing::info_span!("multi_call_settle_upto_permit2",
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
            Ok(())
        }
        StructuredSignature::EOA(signature) => {
            let permit_transfer_from = ISignatureTransfer::PermitTransferFrom {
                permitted: permit_witness_transfer_from.permitted,
                nonce: permit_witness_transfer_from.nonce,
                deadline: permit_witness_transfer_from.deadline,
            };
            let witness = permit_witness_transfer_from.witness;
            let settle_call = upto_permit2_proxy.settle(
                permit_transfer_from,
                authorization.permitted.amount,
                payer,
                witness,
                signature.as_bytes().into(),
            );
            let settle_call_fut = settle_call.call().into_future();
            #[cfg(feature = "telemetry")]
            settle_call_fut
                .instrument(tracing::info_span!("call_settle_upto_permit2",
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
        StructuredSignature::EIP1271(signature) => {
            let permit_transfer_from = ISignatureTransfer::PermitTransferFrom {
                permitted: permit_witness_transfer_from.permitted,
                nonce: permit_witness_transfer_from.nonce,
                deadline: permit_witness_transfer_from.deadline,
            };
            let witness = permit_witness_transfer_from.witness;
            let settle_call =
                upto_permit2_proxy.settle(permit_transfer_from, authorization.permitted.amount, payer, witness, signature);
            let settle_call_fut = settle_call.call().into_future();
            #[cfg(feature = "telemetry")]
            settle_call_fut
                .instrument(tracing::info_span!("call_settle_upto_permit2",
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
    }
}

pub async fn settle_upto_permit2<P, E>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    actual_amount: U256,
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
        verifying_contract: types::PERMIT2_ADDRESS,
    };
    let permit_witness_transfer_from = PermitWitnessTransferFrom {
        permitted: ISignatureTransfer::TokenPermissions {
            token: authorization.permitted.token.into(),
            amount: authorization.permitted.amount,
        },
        spender: types::UPTO_PERMIT2_PROXY_ADDRESS,
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

    let upto_permit2_proxy =
        X402UptoPermit2Proxy::new(types::UPTO_PERMIT2_PROXY_ADDRESS, provider.inner());
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
            let settle_call =
                upto_permit2_proxy.settle(permit_transfer_from, actual_amount, payer, witness, inner.clone());
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
                        tracing::info_span!("call_upto_permit2_proxy_settle.EIP6492.deployed",
                            from = %payer,
                            to = %authorization.witness.to,
                            max_value = %authorization.permitted.amount,
                            actual_value = %actual_amount,
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
                    target: settle_call.target(),
                    callData: settle_call.calldata().clone(),
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
                    .instrument(tracing::info_span!("call_upto_permit2_proxy_settle.EIP6492.counterfactual",
                        from = %payer,
                        to = %authorization.witness.to,
                        max_value = %authorization.permitted.amount,
                        actual_value = %actual_amount,
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
            let settle_call = upto_permit2_proxy.settle(
                permit_transfer_from,
                actual_amount,
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
                .instrument(tracing::info_span!("call_upto_permit2_proxy_settle.EOA",
                    from = %payer,
                    to = %authorization.witness.to,
                    max_value = %authorization.permitted.amount,
                    actual_value = %actual_amount,
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
            let settle_call =
                upto_permit2_proxy.settle(permit_transfer_from, actual_amount, payer, witness, signature.clone());
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
                    tracing::info_span!("call_upto_permit2_proxy_settle.EIP1271",
                        from = %payer,
                        to = %authorization.witness.to,
                        max_value = %authorization.permitted.amount,
                        actual_value = %actual_amount,
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
