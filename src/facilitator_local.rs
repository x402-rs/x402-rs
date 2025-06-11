//! Facilitator implementation for x402 payments using on-chain verification and settlement.
//!
//! This module provides a [`Facilitator`] implementation that validates x402 payment payloads
//! and performs on-chain settlements using ERC-3009 `transferWithAuthorization`.
//!
//! Features include:
//! - EIP-712 signature recovery
//! - ERC-20 balance checks
//! - Contract interaction using Alloy
//! - Network-specific configuration via [`ProviderCache`] and [`USDCDeployment`]

use alloy::network::Ethereum;
use alloy::primitives::{Bytes, FixedBytes, Signature, U256};
use alloy::providers::Provider;
use alloy::sol;
use alloy::sol_types::{Eip712Domain, SolStruct, eip712_domain};
use std::fmt::Debug;
use std::future::IntoFuture;
use std::time::SystemTime;
use tracing::{Instrument, instrument};
use tracing_core::Level;

use crate::facilitator::Facilitator;
use crate::network::{Network, USDCDeployment};
use crate::provider_cache::ProviderCache;
use crate::provider_cache::ProviderMap;
use crate::types::{
    EvmAddress, ExactEvmPayload, ExactEvmPayloadAuthorization, FacilitatorErrorReason,
    MixedAddress, MixedAddressError, PaymentPayload, PaymentRequirements, Scheme, SettleRequest,
    SettleResponse, TransactionHash, TransferWithAuthorization, VerifyRequest, VerifyResponse,
};

/// Represents all possible errors that may occur during verification or settlement of x402 payments.
#[derive(thiserror::Error, Debug)]
pub enum PaymentError {
    /// The scheme (e.g. "exact") declared in the payload is incompatible with the requirements.
    #[error("Incompatible payload scheme (payload: {payload}, requirements: {requirements})")]
    IncompatibleScheme {
        payload: Scheme,
        requirements: Scheme,
    },
    /// The network (e.g. Base) declared in the payload doesn't match the requirements.
    #[error("Incompatible payload network (payload: {payload}, requirements: {requirements})")]
    IncompatibleNetwork {
        payload: Network,
        requirements: Network,
    },
    /// The `pay_to` recipient in the requirements doesn't match the `to` address in the payload.
    #[error("Incompatible payload receivers (payload: {payload}, requirements: {requirements})")]
    IncompatibleReceivers {
        payload: EvmAddress,
        requirements: MixedAddress,
    },
    /// Low-level contract interaction failure (e.g. call failed, method not found).
    #[error(transparent)]
    InvalidContractCall(#[from] alloy::contract::Error),
    /// Error parsing a mixed address into an EVM address.
    #[error(transparent)]
    InvalidAddress(#[from] MixedAddressError),
    /// EIP-712 signature is invalid or mismatched.
    #[error("Invalid signature: {0}")]
    InvalidSignature(String),
    /// The `validAfter`/`validBefore` fields on the authorization are not within bounds.
    #[error("Invalid timing: {0}")]
    InvalidTiming(String),
    /// The network is not supported by this facilitator.
    #[error("Unsupported network: {0}")]
    UnsupportedNetwork(Network),
    /// The payer's on-chain balance is insufficient for the payment.
    #[error("Insufficient funds")]
    InsufficientFunds,
    /// The payload's `value` is not enough to meet the requirements.
    #[error("Insufficient value")]
    InsufficientValue,
    /// Failed to read system clock to check timing.
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

#[derive(Clone, Debug)]
pub struct FacilitatorLocal<P = ProviderCache> {
    pub provider_cache: P,
}

impl<P> FacilitatorLocal<P>
where
    P: ProviderMap<Value: Provider<Ethereum>>,
{
    pub fn new(provider_cache: P) -> Self {
        FacilitatorLocal { provider_cache }
    }

    /// Runs all preconditions needed for a successful payment:
    /// - Valid scheme, network, and receiver.
    /// - Valid time window (validAfter/validBefore).
    /// - Correct EIP-712 domain construction.
    /// - Valid EIP-712 signature.
    /// - Sufficient on-chain balance.
    /// - Sufficient value in payload.
    #[instrument(skip_all, err)]
    async fn assert_valid_payment(
        &self,
        payload: &PaymentPayload,
        payment_requirements: &PaymentRequirements,
    ) -> Result<ValidPaymentResult<&P::Value>, PaymentError> {
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

        let provider = self
            .provider_cache
            .by_network(payload.network)
            .ok_or(PaymentError::UnsupportedNetwork(payload.network))?;
        let asset_address: alloy::primitives::Address = payment_requirements
            .asset
            .clone()
            .try_into()
            .map_err(PaymentError::InvalidAddress)?;
        let contract = USDC::new(asset_address, provider);

        let domain =
            assert_domain(&contract, payload, &asset_address, payment_requirements).await?;
        assert_signature(&payload.payload, &domain)?;

        let amount_required = payment_requirements.max_amount_required.0;
        assert_enough_balance(
            &contract,
            &payload.payload.authorization.from,
            amount_required,
        )
        .await?;
        let value: U256 = payload.payload.authorization.value.into();
        assert_enough_value(&value, &amount_required)?;
        let eip1559 = self.provider_cache.eip1559(payload.network);

        Ok(ValidPaymentResult {
            contract,
            provider,
            eip1559,
        })
    }
}

impl<P> Facilitator for FacilitatorLocal<P>
where
    P: ProviderMap<Value: Provider<Ethereum> + Send + Sync> + Send + Sync,
{
    type Error = PaymentError;

    /// Verifies a proposed x402 payment payload against a passed [`PaymentRequirements`].
    ///
    /// This function validates the signature, timing, receiver match, network, scheme, and on-chain
    /// balance sufficiency for the token. If all checks pass, return a [`VerifyResponse::Valid`].
    ///
    /// Called from the `/verify` HTTP endpoint on the facilitator.
    ///
    /// # Errors
    ///
    /// Returns [`PaymentError`] if any check fails, including:
    /// - scheme/network mismatch,
    /// - receiver mismatch,
    /// - invalid signature,
    /// - expired or future-dated timing,
    /// - insufficient funds,
    /// - unsupported network.
    #[instrument(skip_all, err, fields(chain_id = %request.payment_payload.network.chain_id()))]
    async fn verify(&self, request: &VerifyRequest) -> Result<VerifyResponse, Self::Error> {
        let payload = &request.payment_payload;
        self.assert_valid_payment(payload, &request.payment_requirements)
            .await?;
        Ok(VerifyResponse::valid(payload.payload.authorization.from))
    }

    /// Executes an x402 payment on-chain using ERC-3009 `transferWithAuthorization`.
    ///
    /// This function performs the same validations as `verify`, then sends the authorized transfer
    /// via a smart contract and waits for transaction receipt.
    ///
    /// Called from the `/settle` HTTP endpoint on the facilitator.
    ///
    /// # Errors
    ///
    /// Returns [`PaymentError`] if validation or contract call fails. Transaction receipt is included
    /// in the response on success or failure.
    #[instrument(skip_all, err, fields(chain_id = %request.payment_payload.network.chain_id()))]
    async fn settle(&self, request: &SettleRequest) -> Result<SettleResponse, Self::Error> {
        let payload = &request.payment_payload;
        let payment_requirements = &request.payment_requirements;
        let valid_payment = self
            .assert_valid_payment(payload, payment_requirements)
            .await?;
        let contract = valid_payment.contract;
        let provider = valid_payment.provider;
        let eip1559 = valid_payment.eip1559;

        let from: alloy::primitives::Address = payload.payload.authorization.from.into();
        let to: alloy::primitives::Address = payload.payload.authorization.to.into();
        let value: U256 = payload.payload.authorization.value.into();
        let valid_after: U256 = payload.payload.authorization.valid_after.into();
        let valid_before: U256 = payload.payload.authorization.valid_before.into();
        let nonce = FixedBytes(payload.payload.authorization.nonce.0);
        let signature = Bytes::from(payload.payload.signature.0);

        let tx = contract.transferWithAuthorization_0(
            from,
            to,
            value,
            valid_after,
            valid_before,
            nonce,
            signature.clone(),
        );

        let tx = if eip1559 {
            tx
        } else {
            let gas = provider
                .get_gas_price()
                .await
                .map_err(|e| PaymentError::InvalidContractCall(e.into()))?;
            tx.gas_price(gas)
        };

        let tx = tx
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
                error_reason: Some(FacilitatorErrorReason::InvalidScheme),
                payer: payload.payload.authorization.from.into(),
                transaction: Some(TransactionHash(receipt.transaction_hash.0)),
                network: payload.network,
            })
        }
    }
}

/// Result of a successful x402 payment validation.
///
/// This struct packages all the verified components needed for settlement, including:
/// - an instance of the token contract (`USDCInstance`) ready to execute transfers,
/// - the Ethereum provider used for interacting with the blockchain,
/// - a boolean flag indicating whether the target network supports EIP-1559.
///
/// Used internally to pass validated context from [`FacilitatorLocal::assert_valid_payment`] into [`FacilitatorLocal::settle`].
struct ValidPaymentResult<P> {
    /// An instance of the verified token contract, ready to perform `transferWithAuthorization`.
    contract: USDC::USDCInstance<P>,
    /// The Ethereum provider configured for the target network.
    provider: P,
    /// Whether the network uses EIP-1559-style fee parameters (used to decide gas pricing mode).
    eip1559: bool,
}

/// Checks whether the basic payment requirements are compatible with the payload.
///
/// Verifies the following:
/// - The scheme (e.g. "exact") in the payload matches the required one.
/// - The network (e.g. Base) matches.
/// - The recipient address matches.
///
/// # Errors
/// Returns a [`PaymentError`] if any of these checks fail.
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
    let requirements_receiver_evm: &EvmAddress = &requirements_receiver_mixed
        .clone()
        .try_into()
        .map_err(PaymentError::InvalidAddress)?;
    if payload_receiver_evm != requirements_receiver_evm {
        return Err(PaymentError::IncompatibleReceivers {
            payload: *payload_receiver_evm,
            requirements: requirements_receiver_mixed,
        });
    }
    Ok(())
}

/// Constructs the correct EIP-712 domain for signature verification.
///
/// Resolves the `name` and `version` based on:
/// - Static metadata from [`USDCDeployment`] (if available),
/// - Or by calling `version()` on the token contract if not matched statically.
///
/// # Errors
/// Returns a [`PaymentError::InvalidContractCall`] if the contract call fails.
#[instrument(skip_all, err, fields(
    network = %payload.network,
    asset = %asset_address,
    chain_id = %payload.network.chain_id()
))]
async fn assert_domain<P: Provider<Ethereum>>(
    token_contract: &USDC::USDCInstance<P>,
    payload: &PaymentPayload,
    asset_address: &alloy::primitives::Address,
    requirements: &PaymentRequirements,
) -> Result<Eip712Domain, PaymentError> {
    let usdc = USDCDeployment::by_network(payload.network);
    let name = requirements
        .extra
        .as_ref()
        .and_then(|e| e.get("name")?.as_str().map(str::to_string))
        .unwrap_or_else(|| usdc.eip712.name.clone());
    let chain_id = payload.network.chain_id();
    let version = requirements
        .extra
        .as_ref()
        .and_then(|extra| extra.get("version"))
        .and_then(|version| version.as_str().map(|s| s.to_string()));
    let version = if let Some(extra_version) = version {
        extra_version
    } else if usdc.address == *asset_address {
        usdc.eip712.version.clone()
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

/// Verifies the EIP-712 signature in the payment payload.
///
/// Recovers the signing address and checks it matches the expected `from` address in the payload.
///
/// # Errors
/// Returns a [`PaymentError::InvalidSignature`] if the signature is malformed or does not match.
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

/// Validates that the current time is within the `validAfter` and `validBefore` bounds.
///
/// Adds a 6-second grace buffer when checking expiration to account for latency.
///
/// # Errors
/// Returns [`PaymentError::InvalidTiming`] if the authorization is not yet active or already expired.
/// Returns [`PaymentError::ClockError`] if the system clock cannot be read.
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

/// Checks if the payer has enough on-chain token balance to meet the `maxAmountRequired`.
///
/// Performs an `ERC20.balanceOf()` call using the USDC contract instance.
///
/// # Errors
/// Returns [`PaymentError::InsufficientFunds`] if the balance is too low.
/// Returns [`PaymentError::InvalidContractCall`] if the balance query fails.
#[instrument(skip_all, err, fields(
    sender = %sender,
    max_required = %max_amount_required,
    token_contract = %usdc_contract.address()
))]
async fn assert_enough_balance<P: Provider<Ethereum>>(
    usdc_contract: &USDC::USDCInstance<P>,
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

/// Verifies that the declared `value` in the payload is sufficient for the required amount.
///
/// This is a static check (not on-chain) that compares two numbers.
///
/// # Errors
/// Returns [`PaymentError::InsufficientValue`] if the payload's value is less than required.
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
