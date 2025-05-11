use crate::facilitator::USDC::USDCInstance;
use crate::network::{Network, USDCDeployment};
use crate::provider_cache::{EthereumProvider, ProviderCache};
use crate::types::{
    ErrorReason, EvmAddress, ExactEvmPayload, ExactEvmPayloadAuthorization, MixedAddress,
    PaymentPayload, PaymentRequirements, Scheme, SettleResponse, TransactionHash,
    TransferWithAuthorization, VerifyResponse,
};
use alloy::network::Ethereum;
use alloy::primitives::{AddressError, Bytes, FixedBytes, Signature, U256};
use alloy::providers::Provider;
use alloy::sol;
use alloy::sol_types::{eip712_domain, Eip712Domain, SolStruct};
use std::future::IntoFuture;
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{instrument, Instrument};
use tracing_core::Level;

#[derive(thiserror::Error, Debug)]
pub enum PaymentError {
    #[error("Incompatible payload scheme (payload: {payload}, requirements: {requirements})")]
    IncompatibleScheme {
        payload: Scheme,
        requirements: Scheme,
    },
    #[error("Incompatible payload network (payload: {payload}, requirements: {requirements})")]
    IncompatibleNetwork {
        payload: Network,
        requirements: Network,
    },
    #[error("Incompatible payload receivers (payload: {payload}, requirements: {requirements})")]
    IncompatibleReceivers {
        payload: EvmAddress,
        requirements: MixedAddress,
    },
    #[error(transparent)]
    InvalidContractCall(#[from] alloy::contract::Error),
    #[error(transparent)]
    InvalidAddress(#[from] AddressError),
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),
    #[error("Invalid timing: {0}")]
    InvalidTiming(String),
    #[error("Unsupported network: {0}")]
    UnsupportedNetwork(Network),
    #[error("Insufficient funds")]
    InsufficientFunds,
    #[error("Insufficient value")]
    InsufficientValue,
    #[error("Can not get system clock")]
    ClockError,
}

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    USDC,
    "abi/USDC.json"
);

#[instrument(skip_all, err, fields(chain_id = %payload.network.chain_id()))]
pub async fn verify(
    provider_cache: Arc<ProviderCache>,
    payload: &PaymentPayload,
    payment_requirements: &PaymentRequirements,
) -> Result<VerifyResponse, PaymentError> {
    assert_valid_payment(&provider_cache, payload, payment_requirements).await?;
    Ok(VerifyResponse {
        is_valid: true,
        invalid_reason: None,
        payer: payload.payload.authorization.from,
    })
}

#[instrument(skip_all, err)]
async fn assert_valid_payment<'a>(
    provider_cache: &'a Arc<ProviderCache>,
    payload: &PaymentPayload,
    payment_requirements: &PaymentRequirements,
) -> Result<ValidPaymentResult<'a>, PaymentError> {
    /*
    verification steps:
      - ✅ verify payload version
      - ✅ verify usdc address is correct for the chain
      - ✅ verify permit signature
      - ✅ verify deadline
      - verify nonce is current
      - ✅ verify client has enough funds to cover paymentRequirements.maxAmountRequired
      - ✅ verify value in payload is enough to cover paymentRequirements.maxAmountRequired
      - check min amount is above some threshold we think is reasonable for covering gas
      - verify resource is not already paid for (next version)
      - make Axum automatically return VerificationError as 400 Bad Request without manual match
      */
    assert_requirements(payload, payment_requirements)?;
    assert_time(&payload.payload.authorization)?;

    let provider = provider_cache
        .by_network(payload.network)
        .ok_or_else(|| PaymentError::UnsupportedNetwork(payload.network))?;
    let asset_address: alloy::primitives::Address = payment_requirements
        .asset
        .clone()
        .try_into()
        .map_err(PaymentError::InvalidAddress)?;
    let contract = USDC::new(asset_address, provider);

    let domain = assert_domain(&contract, payload, &asset_address, payment_requirements).await?;
    assert_signature(&payload.payload, &domain)?;

    let amount_required = U256::from(payment_requirements.max_amount_required.0);
    assert_enough_balance(
        &contract,
        &payload.payload.authorization.from,
        amount_required,
    )
    .await?;
    let value: U256 = payload.payload.authorization.value.into();
    assert_enough_value(&value, &amount_required)?;

    Ok(ValidPaymentResult { contract })
}

struct ValidPaymentResult<'a> {
    contract: USDCInstance<&'a Arc<EthereumProvider>>,
}

#[instrument(skip_all, err, fields(chain_id = %payload.network.chain_id()))]
pub async fn settle(
    provider_cache: Arc<ProviderCache>,
    payload: &PaymentPayload,
    payment_requirements: &PaymentRequirements,
) -> Result<SettleResponse, PaymentError> {
    let contract = assert_valid_payment(&provider_cache, payload, payment_requirements)
        .await?
        .contract;

    let from: alloy::primitives::Address = payload.payload.authorization.from.into();
    let to: alloy::primitives::Address = payload.payload.authorization.to.into();
    let value: U256 = payload.payload.authorization.value.into();
    let valid_after: U256 = payload.payload.authorization.valid_after.into();
    let valid_before: U256 = payload.payload.authorization.valid_before.into();
    let nonce = FixedBytes(payload.payload.authorization.nonce.0);
    let signature = Bytes::from(payload.payload.signature.0);
    let tx = contract
        .transferWithAuthorization_0(
            from,
            to,
            value,
            valid_after,
            valid_before,
            nonce,
            signature.clone(),
        )
        .send()
        .instrument(tracing::info_span!("transferWithAuthorization_0",
                from = %from,
                to = %to,
                value = %value,
                valid_after = %valid_after,
                valid_before = %valid_before,
                nonce = %nonce,
                signature = %signature,
                token_contract = %contract.address(),
                otel.kind = "client",
        ))
        .await
        .map_err(PaymentError::InvalidContractCall)?;
    let tx_hash = *tx.tx_hash();
    let receipt = tx
        .get_receipt()
        .into_future()
        .instrument(tracing::info_span!("get_receipt",
                transaction = %tx_hash,
                otel.kind = "client"
        ))
        .await
        .map_err(|e| PaymentError::InvalidContractCall(e.into()))?;
    let success = receipt.status();
    if success {
        tracing::event!(Level::INFO,
            status = "ok",
            tx = %receipt.transaction_hash,
            "transferWithAuthorization_0 succeeded"
        );
        Ok(SettleResponse {
            success: true,
            error_reason: None,
            payer: payload.payload.authorization.from.into(),
            transaction: Some(TransactionHash(receipt.transaction_hash.0)),
            network: payload.network,
        })
    } else {
        tracing::event!(
            Level::WARN,
            status = "failed",
            tx = %receipt.transaction_hash,
            "transferWithAuthorization_0 failed"
        );
        Ok(SettleResponse {
            success: false,
            error_reason: Some(ErrorReason::InvalidScheme),
            payer: payload.payload.authorization.from.into(),
            transaction: Some(TransactionHash(receipt.transaction_hash.0)),
            network: payload.network,
        })
    }
}

#[instrument(skip_all, err)]
fn assert_requirements(
    payload: &PaymentPayload,
    requirements: &PaymentRequirements,
) -> Result<(), PaymentError> {
    if payload.scheme != requirements.scheme {
        return Err(PaymentError::IncompatibleScheme {
            payload: payload.scheme,
            requirements: requirements.scheme,
        });
    }
    if payload.network != requirements.network {
        return Err(PaymentError::IncompatibleNetwork {
            payload: payload.network,
            requirements: requirements.network,
        });
    }
    let payload_receiver_evm = &payload.payload.authorization.to;
    let requirements_receiver_mixed = requirements.pay_to.clone();
    let requirements_receiver_evm: &EvmAddress = &requirements_receiver_mixed.clone().try_into()?;
    if payload_receiver_evm != requirements_receiver_evm {
        return Err(PaymentError::IncompatibleReceivers {
            payload: *payload_receiver_evm,
            requirements: requirements_receiver_mixed,
        });
    }
    Ok(())
}

#[instrument(skip_all, err, fields(
    network = %payload.network,
    asset = %asset_address,
    chain_id = %payload.network.chain_id()
))]
async fn assert_domain<P: Provider<Ethereum>>(
    token_contract: &USDCInstance<P>,
    payload: &PaymentPayload,
    asset_address: &alloy::primitives::Address,
    requirements: &PaymentRequirements,
) -> Result<Eip712Domain, PaymentError> {
    let usdc = USDCDeployment::by_network(&payload.network);
    let name = requirements
        .extra
        .as_ref()
        .and_then(|extra| extra.get("name"))
        .and_then(|name| name.as_str().map(|s| s.to_string()))
        .unwrap_or(usdc.name);
    let chain_id = payload.network.chain_id();
    let version = requirements
        .extra
        .as_ref()
        .and_then(|extra| extra.get("version"))
        .and_then(|version| version.as_str().map(|s| s.to_string()));
    let version = if let Some(extra_version) = version {
        extra_version
    } else {
        token_contract
            .version()
            .call()
            .into_future()
            .instrument(tracing::info_span!(
                "fetch_eip712_version",
                otel.kind = "client",
            ))
            .await
            .map_err(PaymentError::InvalidContractCall)?
    };
    let domain = eip712_domain! {
        name: name,
        version: version,
        chain_id: chain_id,
        verifying_contract: *asset_address,
    };
    Ok(domain)
}

#[instrument(skip_all, err)]
fn assert_signature(payload: &ExactEvmPayload, domain: &Eip712Domain) -> Result<(), PaymentError> {
    // Verify the signature
    let signature = Signature::from_raw_array(&payload.signature.0)
        .map_err(|e| PaymentError::InvalidSignature(format!("{}", e)))?;
    let authorization = &payload.authorization;
    let transfer_with_authorization = TransferWithAuthorization {
        from: authorization.from.0,
        to: authorization.to.0,
        value: authorization.value.into(),
        validAfter: authorization.valid_after.into(),
        validBefore: authorization.valid_before.into(),
        nonce: FixedBytes(authorization.nonce.0),
    };
    let eip712_hash = transfer_with_authorization.eip712_signing_hash(domain);
    let recovered_address = signature
        .recover_address_from_prehash(&eip712_hash)
        .map_err(|e| PaymentError::InvalidSignature(format!("{}", e)))?;
    let expected_address = authorization.from.0;
    if recovered_address != expected_address {
        Err(PaymentError::InvalidSignature(format!(
            "Address mismatch: recovered: {} expected: {}",
            recovered_address, expected_address
        )))
    } else {
        Ok(())
    }
}

#[instrument(skip_all, err)]
fn assert_time(authorization: &ExactEvmPayloadAuthorization) -> Result<(), PaymentError> {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| PaymentError::ClockError)?
        .as_secs();
    let valid_before = authorization.valid_before.0;
    if valid_before < now + 6 {
        return Err(PaymentError::InvalidTiming(format!(
            "Expired: now {} > valid_before {}",
            now + 6,
            valid_before
        )));
    }
    let valid_after = authorization.valid_after.0;
    if valid_after > now {
        return Err(PaymentError::InvalidTiming(format!(
            "Not active yet: valid_after {} > now {}",
            valid_after, now
        )));
    }
    Ok(())
}

#[instrument(skip_all, err, fields(
    sender = %sender,
    max_required = %max_amount_required,
    token_contract = %usdc_contract.address()
))]
async fn assert_enough_balance<P: Provider<Ethereum>>(
    usdc_contract: &USDCInstance<P>,
    sender: &EvmAddress,
    max_amount_required: U256,
) -> Result<(), PaymentError> {
    let balance = usdc_contract
        .balanceOf(sender.0)
        .call()
        .into_future()
        .instrument(tracing::info_span!(
            "fetch_token_balance",
            token_contract = %usdc_contract.address(),
            sender = %sender,
            otel.kind = "client"
        ))
        .await
        .map_err(PaymentError::InvalidContractCall)?;

    if balance < max_amount_required {
        Err(PaymentError::InsufficientFunds)
    } else {
        Ok(())
    }
}

#[instrument(skip_all, err, fields(
    sent = %sent,
    max_amount_required = %max_amount_required
))]
fn assert_enough_value(sent: &U256, max_amount_required: &U256) -> Result<(), PaymentError> {
    if sent < max_amount_required {
        Err(PaymentError::InsufficientValue)
    } else {
        Ok(())
    }
}
