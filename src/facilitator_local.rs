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

use alloy::contract::SolCallBuilder;
use alloy::network::Ethereum;
use alloy::primitives::{Bytes, FixedBytes, Signature, U256};
use alloy::providers::Provider;
use alloy::sol;
use alloy::sol_types::{Eip712Domain, SolStruct, eip712_domain};
use std::fmt::Debug;
use std::future::IntoFuture;
use std::time::{SystemTime, SystemTimeError};
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
    ClockError(#[source] SystemTimeError),
}

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    USDC,
    "abi/USDC.json"
);

/// A concrete [`Facilitator`] implementation that verifies and settles x402 payments
/// using a network-aware provider cache.
///
/// This type is generic over the [`ProviderMap`] implementation used to access EVM providers,
/// which enables testing or customization beyond the default [`ProviderCache`].
#[derive(Clone, Debug)]
pub struct FacilitatorLocal<P = ProviderCache> {
    pub provider_cache: P,
}

/// A prepared call to `transferWithAuthorization` (ERC-3009) including all derived fields.
///
/// This struct wraps the assembled call builder, making it reusable across verification
/// (`.call()`) and settlement (`.send()`) flows, along with context useful for tracing/logging.
///
/// This is created by [`FacilitatorLocal::transferWithAuthorization_0`].
pub struct TransferWithAuthorization0Call<P> {
    /// The prepared call builder that can be `.call()`ed or `.send()`ed.
    pub tx: SolCallBuilder<P, USDC::transferWithAuthorization_0Call>,
    /// The sender (`from`) address for the authorization.
    pub from: alloy::primitives::Address,
    /// The recipient (`to`) address for the authorization.
    pub to: alloy::primitives::Address,
    /// The amount to transfer (value).
    pub value: U256,
    /// Start of the validity window (inclusive).
    pub valid_after: U256,
    /// End of the validity window (exclusive).
    pub valid_before: U256,
    /// 32-byte authorization nonce (prevents replay).
    pub nonce: FixedBytes<32>,
    /// EIP-712 signature for the transfer authorization.
    pub signature: Bytes,
    /// Address of the token contract used for this transfer.
    pub contract_address: alloy::primitives::Address,
}

impl<P> FacilitatorLocal<P>
where
    P: ProviderMap<Value: Provider<Ethereum>>,
{
    /// Creates a new [`FacilitatorLocal`] with the given provider cache.
    ///
    /// The provider cache is used to resolve the appropriate EVM provider for each payment's target network.
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
    ) -> Result<USDC::USDCInstance<&P::Value>, PaymentError> {
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

        let amount_required = payment_requirements.max_amount_required;
        assert_enough_balance(
            &contract,
            &payload.payload.authorization.from,
            amount_required,
        )
        .await?;
        let value: U256 = payload.payload.authorization.value.into();
        assert_enough_value(&value, &amount_required)?;
        Ok(contract)
    }

    /// Constructs a full `transferWithAuthorization` call for a verified payment payload.
    ///
    /// This function prepares the transaction builder with gas pricing adapted to the network's
    /// capabilities (EIP-1559 or legacy), and packages it together with signature metadata
    /// into a [`TransferWithAuthorization0Call`] structure.
    ///
    /// This function does not perform any validation — it assumes inputs are already checked.
    #[allow(non_snake_case)]
    async fn transferWithAuthorization_0<'a>(
        &self,
        contract: &'a USDC::USDCInstance<&'a P::Value>,
        payload: &PaymentPayload,
    ) -> Result<TransferWithAuthorization0Call<&'a &'a P::Value>, PaymentError> {
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
        let eip1559 = self.provider_cache.eip1559(payload.network);
        let tx = if eip1559 {
            tx
        } else {
            let provider = contract.provider();
            let gas: u128 = provider
                .get_gas_price()
                .instrument(tracing::info_span!("get_gas_price"))
                .await
                .map_err(|e| PaymentError::InvalidContractCall(e.into()))?;
            tx.gas_price(gas)
        };
        Ok(TransferWithAuthorization0Call {
            tx,
            from,
            to,
            value,
            valid_after,
            valid_before,
            nonce,
            signature,
            contract_address: *contract.address(),
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
        let contract = self
            .assert_valid_payment(payload, &request.payment_requirements)
            .await?;
        let transfer_call = self.transferWithAuthorization_0(&contract, payload).await?;
        transfer_call
            .tx
            .call()
            .into_future()
            .instrument(tracing::info_span!("call_transferWithAuthorization_0",
                    from = %transfer_call.from,
                    to = %transfer_call.to,
                    value = %transfer_call.value,
                    valid_after = %transfer_call.valid_after,
                    valid_before = %transfer_call.valid_before,
                    nonce = %transfer_call.nonce,
                    signature = %transfer_call.signature,
                    token_contract = %transfer_call.contract_address,
                    otel.kind = "client",
            ))
            .await
            .map_err(PaymentError::InvalidContractCall)?;
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
        let contract = self
            .assert_valid_payment(payload, payment_requirements)
            .await?;

        let transfer_call = self.transferWithAuthorization_0(&contract, payload).await?;
        let tx = transfer_call
            .tx
            .send()
            .instrument(tracing::info_span!("transferWithAuthorization_0",
                    from = %transfer_call.from,
                    to = %transfer_call.to,
                    value = %transfer_call.value,
                    valid_after = %transfer_call.valid_after,
                    valid_before = %transfer_call.valid_before,
                    nonce = %transfer_call.nonce,
                    signature = %transfer_call.signature,
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
    } else if usdc.address() == *asset_address {
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
        .map_err(PaymentError::ClockError)?
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
