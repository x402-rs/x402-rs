use alloy_primitives::{Address, B256, Bytes, TxHash, U256};
use alloy_provider::bindings::IMulticall3;
use alloy_provider::{MULTICALL3_ADDRESS, MulticallItem, Provider};
use alloy_rpc_types_eth::TransactionReceipt;
use alloy_sol_types::{SolCall, SolStruct, eip712_domain};
use x402_types::chain::ChainProviderOps;
use x402_types::proto::{PaymentVerificationError, v2};
use x402_types::scheme::X402SchemeFacilitatorError;

use super::eip2612::{self, Permit2PaymentPayloadExt};

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
use crate::v2_eip155_exact::eip3009::assert_requirements_match;
use crate::v2_eip155_exact::types::{
    ISignatureTransfer, Permit2PaymentPayload, Permit2PaymentRequirements,
    PermitWitnessTransferFrom, X402ExactPermit2Proxy, x402ExactPermit2Proxy,
};

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

    // Check if the client provided EIP-2612 gas-sponsoring extension data
    let eip2612_gas_sponsoring = payment_payload.eip2612_gas_sponsoring()?;

    if let Some(eip2612_gas_sponsoring) = &eip2612_gas_sponsoring {
        eip2612::assert_eip2612_offchain_valid(eip2612_gas_sponsoring, payment_payload)?;
        eip2612::assert_onchain_exact_permit2_with_eip2612(
            provider.inner(),
            provider.chain(),
            payment_payload,
            eip2612_gas_sponsoring,
        )
        .await?;
    } else {
        assert_onchain_exact_permit2(provider.inner(), provider.chain(), payment_payload).await?;
    }

    Ok(v2::VerifyResponse::valid(payer.to_string()))
}

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn settle_permit2_payment<P, E>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
) -> Result<v2::SettleResponse, X402SchemeFacilitatorError>
where
    P: Eip155MetaTransactionProvider<Error = E> + ChainProviderOps,
    Eip155ExactError: From<E>,
{
    // 1. Verify offchain constraints
    assert_offchain_valid(payment_payload, payment_requirements)?;

    // Check if the client provided EIP-2612 gas-sponsoring extension data
    let eip2612_gas_sponsoring = payment_payload.eip2612_gas_sponsoring()?;

    // 2. Try settle (with or without EIP-2612 permit)
    let tx_hash = if let Some(eip2612_gas_sponsoring) = &eip2612_gas_sponsoring {
        eip2612::assert_eip2612_offchain_valid(eip2612_gas_sponsoring, payment_payload)?;
        eip2612::settle_exact_permit2_with_eip2612(
            provider,
            payment_payload,
            eip2612_gas_sponsoring,
        )
        .await?
    } else {
        settle_exact_permit2(provider, payment_payload).await?
    };

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

/// Generic pre-computed Permit2 settlement data.
///
/// This struct holds the shared preparation work — EIP-712 hash, parsed signature,
/// and the contract call arguments — used by both verify and settle code paths.
///
/// Both type parameters encode how the proxy contract types differ between schemes:
/// - `TPermitTransferFrom`: the `PermitTransferFrom` type from the scheme's `sol!`-generated proxy
/// - `TWitness`: the witness struct from the scheme's `sol!`-generated proxy
///
/// NOTE: `TPermitTransferFrom` types from exact and upto are structurally identical but
/// nominally distinct (separate `sol!` invocations). Unifying them requires merging the
/// `sol!` definitions in the types modules — a separate refactor.
pub struct PreparedPermit2<TPermitTransferFrom, TWitness> {
    pub payer: Address,
    pub eip712_hash: B256,
    pub structured_signature: StructuredSignature,
    pub permit_transfer_from: TPermitTransferFrom,
    pub witness: TWitness,
}

/// Type alias for the exact-scheme prepared data.
pub type PreparedExactPermit2 =
    PreparedPermit2<ISignatureTransfer::PermitTransferFrom, x402ExactPermit2Proxy::Witness>;

impl PreparedExactPermit2 {
    /// Build the shared Permit2 data needed for both verify and settle operations.
    ///
    /// Constructs the EIP-712 domain, `PermitWitnessTransferFrom` struct, computes the
    /// signing hash, and parses the structured signature — eliminating the duplicated
    /// prep block that would otherwise appear in every verify/settle function.
    pub fn try_new(
        chain_reference: &Eip155ChainReference,
        payment_payload: &Permit2PaymentPayload,
    ) -> Result<Self, Eip155ExactError> {
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
        let permit_transfer_from = ISignatureTransfer::PermitTransferFrom {
            permitted: permit_witness_transfer_from.permitted,
            nonce: permit_witness_transfer_from.nonce,
            deadline: permit_witness_transfer_from.deadline,
        };
        let witness = permit_witness_transfer_from.witness;

        Ok(Self {
            payer,
            eip712_hash,
            structured_signature,
            permit_transfer_from,
            witness,
        })
    }
}

/// Generic Permit2 settlement execution.
///
/// This function handles the common settlement dispatch logic for all Permit2-based
/// settlements. It matches on the signature type (EIP6492 / EOA / EIP1271) and dispatches
/// accordingly:
/// - EIP6492: Checks if the wallet is deployed, sends directly or via Multicall3
/// - EOA: Sends directly
/// - EIP1271: Sends directly
///
/// The `build_call` closure captures everything needed and only receives signature bytes.
pub async fn execute_permit2_settlement<P, E, Inner, BuildCall>(
    provider: &P,
    payer: Address,
    structured_signature: StructuredSignature,
    build_call: BuildCall,
) -> Result<TxHash, Eip155ExactError>
where
    P: Eip155MetaTransactionProvider<Error = E, Inner = Inner> + ChainProviderOps,
    Inner: Provider,
    Eip155ExactError: From<E>,
    BuildCall: FnOnce(Bytes) -> MetaTransaction,
{
    let receipt: TransactionReceipt = match structured_signature {
        StructuredSignature::EIP6492 {
            factory,
            factory_calldata,
            inner,
            original: _,
        } => {
            let is_contract_deployed = is_contract_deployed(provider.inner(), &payer).await?;
            let settle_call = build_call(inner.clone());
            if is_contract_deployed {
                let tx_fut = Eip155MetaTransactionProvider::send_transaction(provider, settle_call);
                #[cfg(feature = "telemetry")]
                let receipt = tx_fut
                    .instrument(tracing::info_span!(
                        "call_permit2_proxy_settle.EIP6492.deployed"
                    ))
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
                let transfer_with_authorization_call = IMulticall3::Call3 {
                    allowFailure: false,
                    target: settle_call.to,
                    callData: settle_call.calldata,
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
                    .instrument(tracing::info_span!(
                        "call_permit2_proxy_settle.EIP6492.counterfactual"
                    ))
                    .await?;
                #[cfg(not(feature = "telemetry"))]
                let receipt = tx_fut.await?;
                receipt
            }
        }
        StructuredSignature::EOA(signature) => {
            let settle_call = build_call(signature.as_bytes().into());
            let tx_fut = Eip155MetaTransactionProvider::send_transaction(provider, settle_call);
            #[cfg(feature = "telemetry")]
            let receipt = tx_fut
                .instrument(tracing::info_span!("call_permit2_proxy_settle.EOA"))
                .await?;
            #[cfg(not(feature = "telemetry"))]
            let receipt = tx_fut.await?;
            receipt
        }
        StructuredSignature::EIP1271(signature) => {
            let settle_call = build_call(signature);
            let tx_fut = Eip155MetaTransactionProvider::send_transaction(provider, settle_call);
            #[cfg(feature = "telemetry")]
            let receipt = tx_fut
                .instrument(tracing::info_span!("call_permit2_proxy_settle.EIP1271"))
                .await?;
            #[cfg(not(feature = "telemetry"))]
            let receipt = tx_fut.await?;
            receipt
        }
    };
    tx_hash_from_receipt(&receipt)
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
pub async fn assert_onchain_exact_permit2<P: Provider>(
    provider: &P,
    chain_reference: &Eip155ChainReference,
    payment_payload: &Permit2PaymentPayload,
) -> Result<(), Eip155ExactError> {
    let authorization = &payment_payload.payload.permit_2_authorization;
    let required_amount = payment_payload.accepted.amount;
    let asset_address = payment_payload.accepted.asset.0;

    let token_contract = IERC20::new(asset_address, provider);

    // Allowance from payer to Permit2 contract is enough
    let onchain_allowance_fut =
        assert_onchain_allowance(&token_contract, authorization.from.0, required_amount);
    // User balance is enough
    let onchain_balance_fut =
        assert_onchain_balance(&token_contract, authorization.from.0, required_amount);
    tokio::try_join!(onchain_allowance_fut, onchain_balance_fut)?;

    // ... and below is a check if we can do the settle

    let PreparedExactPermit2 {
        payer,
        eip712_hash,
        structured_signature,
        permit_transfer_from,
        witness,
    } = PreparedExactPermit2::try_new(chain_reference, payment_payload)?;

    let exact_permit2_proxy = X402ExactPermit2Proxy::new(EXACT_PERMIT2_PROXY_ADDRESS, provider);
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
            let settle_call =
                exact_permit2_proxy.settle(permit_transfer_from, payer, witness, inner);
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
            Ok(())
        }
        StructuredSignature::EOA(signature) => {
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
            Ok(())
        }
        StructuredSignature::EIP1271(signature) => {
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
            Ok(())
        }
    }
}

pub async fn settle_exact_permit2<P, E>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
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

    let build_call = move |sig_bytes: Bytes| {
        let inner = provider.inner();
        let exact_permit2_proxy = X402ExactPermit2Proxy::new(EXACT_PERMIT2_PROXY_ADDRESS, inner);
        let call = exact_permit2_proxy.settle(permit_transfer_from, payer, witness, sig_bytes);
        MetaTransaction::new(call.target(), call.calldata().clone())
    };

    execute_permit2_settlement(provider, payer, structured_signature, build_call).await
}
