use alloy_primitives::{Bytes, TxHash, U256};
use alloy_provider::{MulticallItem, Provider};
use alloy_sol_types::{SolStruct, eip712_domain};
use x402_types::chain::ChainProviderOps;
use x402_types::proto::{PaymentVerificationError, v2};
use x402_types::scheme::X402SchemeFacilitatorError;

#[cfg(feature = "telemetry")]
use tracing::Instrument;
#[cfg(feature = "telemetry")]
use tracing::instrument;

use crate::chain::erc20::IERC20;
use crate::chain::permit2::{PERMIT2_ADDRESS, UPTO_PERMIT2_PROXY_ADDRESS};
use crate::chain::{Eip155ChainReference, Eip155MetaTransactionProvider, MetaTransaction};
use crate::v1_eip155_exact::{
    Eip155ExactError, StructuredSignature, VALIDATOR_ADDRESS, Validator6492, assert_time,
};
use crate::v2_eip155_exact::facilitator::permit2::{
    PreparedPermit2, assert_onchain_allowance, assert_onchain_balance, execute_permit2_settlement,
};
use crate::v2_eip155_upto::types;
use crate::v2_eip155_upto::types::{
    ISignatureTransfer, Permit2PaymentPayload, Permit2PaymentRequirements,
    PermitWitnessTransferFrom, UptoSettleResponse, X402UptoPermit2Proxy, x402BasePermit2Proxy,
};

/// Type alias for the upto-scheme prepared data.
pub type PreparedUptoPermit2 =
    PreparedPermit2<ISignatureTransfer::PermitTransferFrom, x402BasePermit2Proxy::Witness>;

impl PreparedUptoPermit2 {
    /// Build the shared Permit2 data needed for both verify and settle operations (upto scheme).
    ///
    /// Constructs the EIP-712 domain, `PermitWitnessTransferFrom` struct, computes the
    /// signing hash, and parses the structured signature.
    pub fn try_new(
        chain_reference: &Eip155ChainReference,
        payment_payload: &Permit2PaymentPayload,
    ) -> Result<Self, Eip155ExactError> {
        let authorization = &payment_payload.payload.permit_2_authorization;
        let payer = authorization.from.0;

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
            spender: UPTO_PERMIT2_PROXY_ADDRESS,
            nonce: authorization.nonce,
            deadline: U256::from(authorization.deadline.as_secs()),
            witness: x402BasePermit2Proxy::Witness {
                to: authorization.witness.to.into(),
                validAfter: U256::from(authorization.witness.valid_after.as_secs()),
                extra: Default::default(),
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

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn verify_permit2_payment<P: Eip155MetaTransactionProvider + ChainProviderOps>(
    provider: &P,
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
) -> Result<v2::VerifyResponse, Eip155ExactError> {
    // 1. Verify offchain constraints
    let required_amount = assert_offchain_valid_verify(payment_payload, payment_requirements)?;

    // 2. Verify onchain constraints
    let authorization = &payment_payload.payload.permit_2_authorization;
    let payer = authorization.from;
    assert_onchain_upto_permit2(
        provider.inner(),
        provider.chain(),
        payment_payload,
        required_amount,
    )
    .await?;

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
) -> Result<UptoSettleResponse, X402SchemeFacilitatorError>
where
    P: Eip155MetaTransactionProvider<Error = E> + ChainProviderOps,
    Eip155ExactError: From<E>,
{
    // 1. Verify offchain constraints
    let required_amount = assert_offchain_valid_settle(payment_payload, payment_requirements)?;

    let authorization = &payment_payload.payload.permit_2_authorization;
    let payer = authorization.from;

    // 2. Handle zero settlement - no on-chain transaction needed
    // Allowing $0 settlements means unused authorizations naturally expire without on-chain transactions, reducing gas costs and blockchain bloat
    // TODO Document this
    if required_amount.is_zero() {
        let network = &payment_payload.accepted.network;
        return Ok(UptoSettleResponse::success(
            payer.to_string(),
            String::new(), // Empty transaction for $0 settlement
            network.to_string(),
            "0".to_string(),
        ));
    }

    // 3. Execute settlement
    let tx_hash = settle_upto_permit2(provider, payment_payload, required_amount).await?;
    let payer = authorization.from;
    let network = &payment_payload.accepted.network;

    Ok(UptoSettleResponse::success(
        payer.to_string(),
        tx_hash.to_string(),
        network.to_string(),
        required_amount.to_string(),
    ))
}

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub fn assert_offchain_valid_verify(
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
) -> Result<U256, PaymentVerificationError> {
    assert_offchain_valid(payment_payload, payment_requirements)?;
    // Authorized amount must EQUAL the required amount (client authorizes exact max)
    // The server can then settle for any amount <= this max
    let authorization = &payment_payload.payload.permit_2_authorization;
    let accepted_amount = payment_payload.accepted.amount;
    if authorization.permitted.amount != accepted_amount {
        return Err(PaymentVerificationError::InvalidPaymentAmount);
    }
    Ok(accepted_amount)
}

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub fn assert_offchain_valid_settle(
    payment_payload: &Permit2PaymentPayload,
    payment_requirements: &Permit2PaymentRequirements,
) -> Result<U256, PaymentVerificationError> {
    assert_offchain_valid(payment_payload, payment_requirements)?;
    // Authorized amount must EQUAL the required amount (client authorizes exact max)
    // The server can then settle for any amount <= this max
    let authorization = &payment_payload.payload.permit_2_authorization;
    let permitted_amount = authorization.permitted.amount;
    let accepted = &payment_payload.accepted;
    if permitted_amount != accepted.amount {
        return Err(PaymentVerificationError::InvalidPaymentAmount);
    }
    let amount_to_settle = payment_requirements.amount;
    if permitted_amount < amount_to_settle {
        return Err(PaymentVerificationError::InvalidPaymentAmount);
    }
    Ok(amount_to_settle)
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
    if authorization.spender.0 != UPTO_PERMIT2_PROXY_ADDRESS {
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

    // Same token
    if authorization.permitted.token != accepted.asset {
        return Err(PaymentVerificationError::AssetMismatch);
    }
    Ok(())
}

#[cfg_attr(feature = "telemetry", instrument(skip_all, err))]
pub async fn assert_onchain_upto_permit2<P: Provider>(
    provider: &P,
    chain_reference: &Eip155ChainReference,
    payment_payload: &Permit2PaymentPayload,
    required_amount: U256,
) -> Result<(), Eip155ExactError> {
    let authorization = &payment_payload.payload.permit_2_authorization;
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
    // For upto, we simulate with the max amount (worst case)

    let PreparedUptoPermit2 {
        payer,
        eip712_hash,
        structured_signature,
        permit_transfer_from,
        witness,
    } = PreparedUptoPermit2::try_new(chain_reference, payment_payload)?;

    let upto_permit2_proxy = X402UptoPermit2Proxy::new(UPTO_PERMIT2_PROXY_ADDRESS, provider);
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
            // For verification, simulate with max amount
            let settle_call = upto_permit2_proxy.settle(
                permit_transfer_from,
                authorization.permitted.amount,
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
            let settle_call = upto_permit2_proxy.settle(
                permit_transfer_from,
                authorization.permitted.amount,
                payer,
                witness,
                signature,
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
    let PreparedUptoPermit2 {
        payer,
        eip712_hash: _,
        structured_signature,
        permit_transfer_from,
        witness,
    } = PreparedUptoPermit2::try_new(provider.chain(), payment_payload)?;

    let build_call = move |sig_bytes: Bytes| {
        let inner = provider.inner();
        let upto_permit2_proxy = X402UptoPermit2Proxy::new(UPTO_PERMIT2_PROXY_ADDRESS, inner);
        let call = upto_permit2_proxy.settle(
            permit_transfer_from,
            actual_amount,
            payer,
            witness,
            sig_bytes,
        );
        MetaTransaction::new(call.target(), call.calldata().clone())
    };

    execute_permit2_settlement(provider, payer, structured_signature, build_call).await
}
