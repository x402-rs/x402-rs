use alloy::contract::SolCallBuilder;
use alloy::network::EthereumWallet;
use alloy::primitives::{Bytes, FixedBytes, Signature, U256};
use alloy::providers::fillers::{
    BlobGasFiller, ChainIdFiller, FillProvider, GasFiller, JoinFill, NonceFiller, WalletFiller,
};
use alloy::providers::{Identity, Provider, RootProvider, WalletProvider};
use alloy::sol;
use alloy::sol_types::{Eip712Domain, SolStruct, eip712_domain};
use tracing::{Instrument, instrument};
use tracing_core::Level;

use crate::chain::{FacilitatorLocalError, NetworkProviderOps};
use crate::facilitator::Facilitator;
use crate::network::{Network, USDCDeployment};
use crate::timestamp::UnixTimestamp;
use crate::types::{
    EvmAddress, EvmSignature, ExactEvmPayload, ExactPaymentPayload, FacilitatorErrorReason,
    HexEncodedNonce, MixedAddress, PaymentPayload, PaymentRequirements, Scheme, SettleRequest,
    SettleResponse, SupportedPaymentKind, SupportedPaymentKindsResponse, TokenAmount,
    TransactionHash, TransferWithAuthorization, VerifyRequest, VerifyResponse, X402Version,
};

sol!(
    #[allow(missing_docs)]
    #[allow(clippy::too_many_arguments)]
    #[sol(rpc)]
    USDC,
    "abi/USDC.json"
);

/// The fully composed Ethereum provider type used in this project.
///
/// Combines multiple filler layers for gas, nonce, chain ID, blob gas, and wallet signing,
/// and wraps a [`RootProvider`] for actual JSON-RPC communication.
pub type InnerProvider = FillProvider<
    JoinFill<
        JoinFill<
            Identity,
            JoinFill<GasFiller, JoinFill<BlobGasFiller, JoinFill<NonceFiller, ChainIdFiller>>>,
        >,
        WalletFiller<EthereumWallet>,
    >,
    RootProvider,
>;

#[derive(Clone, Debug)]
pub struct EvmChain {
    pub network: Network,
    pub chain_id: u64,
}

impl EvmChain {
    pub fn new(network: Network, chain_id: u64) -> Self {
        Self { network, chain_id }
    }
}

impl TryFrom<Network> for EvmChain {
    type Error = FacilitatorLocalError;

    fn try_from(value: Network) -> Result<Self, Self::Error> {
        match value {
            Network::BaseSepolia => Ok(EvmChain::new(value, 84532)),
            Network::Base => Ok(EvmChain::new(value, 8453)),
            Network::XdcMainnet => Ok(EvmChain::new(value, 50)),
            Network::AvalancheFuji => Ok(EvmChain::new(value, 43113)),
            Network::Avalanche => Ok(EvmChain::new(value, 43114)),
            Network::Solana => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
            Network::SolanaDevnet => Err(FacilitatorLocalError::UnsupportedNetwork(None)),
        }
    }
}

pub struct ExactEvmPayment {
    #[allow(dead_code)] // Just in case.
    pub chain: EvmChain,
    pub from: EvmAddress,
    pub to: EvmAddress,
    pub value: TokenAmount,
    pub valid_after: UnixTimestamp,
    pub valid_before: UnixTimestamp,
    pub nonce: HexEncodedNonce,
    pub signature: EvmSignature,
}

#[derive(Clone, Debug)]
pub struct EvmProvider {
    inner: InnerProvider,
    eip1559: bool,
    chain: EvmChain,
}

impl EvmProvider {
    pub fn try_new(
        inner: InnerProvider,
        eip1559: bool,
        network: Network,
    ) -> Result<Self, FacilitatorLocalError> {
        let chain = EvmChain::try_from(network)?;
        Ok(Self {
            inner,
            eip1559,
            chain,
        })
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
        requirements: &PaymentRequirements,
    ) -> Result<(USDC::USDCInstance<&InnerProvider>, ExactEvmPayment), FacilitatorLocalError> {
        let payment_payload = match payload.payload {
            ExactPaymentPayload::Evm(payload) => payload,
            ExactPaymentPayload::Solana(_) => {
                return Err(FacilitatorLocalError::UnsupportedNetwork(None));
            }
        };
        let payer = payment_payload.authorization.from;
        if payload.network != self.network() {
            return Err(FacilitatorLocalError::NetworkMismatch(
                Some(payer.into()),
                self.network(),
                payload.network,
            ));
        }
        if requirements.network != self.network() {
            return Err(FacilitatorLocalError::NetworkMismatch(
                Some(payer.into()),
                self.network(),
                requirements.network,
            ));
        }
        if payload.scheme != requirements.scheme {
            return Err(FacilitatorLocalError::SchemeMismatch(
                Some(payer.into()),
                requirements.scheme,
                payload.scheme,
            ));
        }
        let payload_to: EvmAddress = payment_payload.authorization.to;
        let requirements_to: EvmAddress = requirements
            .pay_to
            .clone()
            .try_into()
            .map_err(|e| FacilitatorLocalError::InvalidAddress(format!("{e:?}")))?;
        if payload_to != requirements_to {
            return Err(FacilitatorLocalError::ReceiverMismatch(
                payer.into(),
                payload_to.to_string(),
                requirements_to.to_string(),
            ));
        }
        let valid_after = payment_payload.authorization.valid_after;
        let valid_before = payment_payload.authorization.valid_before;
        assert_time(payer.into(), valid_after, valid_before)?;
        let asset_address = requirements
            .asset
            .clone()
            .try_into()
            .map_err(|e| FacilitatorLocalError::InvalidAddress(format!("{e:?}")))?;
        let contract = USDC::new(asset_address, &self.inner);

        let domain = self
            .assert_domain(&contract, payload, &asset_address, requirements)
            .await?;
        assert_signature(payer.into(), &payment_payload, &domain)?;

        let amount_required = requirements.max_amount_required.0;
        assert_enough_balance(
            &contract,
            &payment_payload.authorization.from,
            amount_required,
        )
        .await?;
        let value: U256 = payment_payload.authorization.value.into();
        assert_enough_value(&payer, &value, &amount_required)?;

        let payment = ExactEvmPayment {
            chain: self.chain.clone(),
            from: payment_payload.authorization.from,
            to: payment_payload.authorization.to,
            value: payment_payload.authorization.value,
            valid_after: payment_payload.authorization.valid_after,
            valid_before: payment_payload.authorization.valid_before,
            nonce: payment_payload.authorization.nonce,
            signature: payment_payload.signature,
        };

        Ok((contract, payment))
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
        contract: &'a USDC::USDCInstance<&'a InnerProvider>,
        payment: &ExactEvmPayment,
    ) -> Result<TransferWithAuthorization0Call<&'a &'a InnerProvider>, FacilitatorLocalError> {
        let from: alloy::primitives::Address = payment.from.into();
        let to: alloy::primitives::Address = payment.to.into();
        let value: U256 = payment.value.into();
        let valid_after: U256 = payment.valid_after.into();
        let valid_before: U256 = payment.valid_before.into();
        let nonce = FixedBytes(payment.nonce.0);
        let signature = Bytes::from(payment.signature.0);
        let tx = contract.transferWithAuthorization_0(
            from,
            to,
            value,
            valid_after,
            valid_before,
            nonce,
            signature.clone(),
        );
        let tx = if self.eip1559 {
            tx
        } else {
            let provider = contract.provider();
            let gas: u128 = provider
                .get_gas_price()
                .instrument(tracing::info_span!("get_gas_price"))
                .await
                .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?;
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
        asset = %asset_address
    ))]
    async fn assert_domain(
        &self,
        token_contract: &USDC::USDCInstance<&InnerProvider>,
        payload: &PaymentPayload,
        asset_address: &alloy::primitives::Address,
        requirements: &PaymentRequirements,
    ) -> Result<Eip712Domain, FacilitatorLocalError> {
        let usdc = USDCDeployment::by_network(payload.network);
        let name = requirements
            .extra
            .as_ref()
            .and_then(|e| e.get("name")?.as_str().map(str::to_string))
            .or_else(|| usdc.eip712.clone().map(|e| e.name))
            .ok_or(FacilitatorLocalError::UnsupportedNetwork(None))?;
        let chain_id = self.chain.chain_id;
        let version = requirements
            .extra
            .as_ref()
            .and_then(|extra| extra.get("version"))
            .and_then(|version| version.as_str().map(|s| s.to_string()));
        let version = if let Some(extra_version) = version {
            Some(extra_version)
        } else if usdc.address() == (*asset_address).into() {
            usdc.eip712.clone().map(|e| e.version)
        } else {
            None
        };
        let version = if let Some(version) = version {
            version
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
                .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?
        };
        let domain = eip712_domain! {
            name: name,
            version: version,
            chain_id: chain_id,
            verifying_contract: *asset_address,
        };
        Ok(domain)
    }
}

impl NetworkProviderOps for EvmProvider {
    fn signer_address(&self) -> MixedAddress {
        self.inner.default_signer_address().into()
    }

    fn network(&self) -> Network {
        self.chain.network
    }
}

impl Facilitator for EvmProvider {
    type Error = FacilitatorLocalError;

    async fn verify(&self, request: &VerifyRequest) -> Result<VerifyResponse, Self::Error> {
        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;
        let (contract, payment) = self.assert_valid_payment(payload, requirements).await?;

        let transfer_call = self
            .transferWithAuthorization_0(&contract, &payment)
            .await?;
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
            .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?;
        Ok(VerifyResponse::valid(payment.from.into()))
    }

    async fn settle(&self, request: &SettleRequest) -> Result<SettleResponse, Self::Error> {
        let payload = &request.payment_payload;
        let requirements = &request.payment_requirements;
        let (contract, payment) = self.assert_valid_payment(payload, requirements).await?;
        let transfer_call = self
            .transferWithAuthorization_0(&contract, &payment)
            .await?;
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
                    token_contract = %transfer_call.contract_address,
                    otel.kind = "client",
            ))
            .await
            .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?;
        let tx_hash = *tx.tx_hash();
        let receipt = tx
            .get_receipt()
            .into_future()
            .instrument(tracing::info_span!("get_receipt",
                    transaction = %tx_hash,
                    otel.kind = "client"
            ))
            .await
            .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?;
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
                payer: payment.from.into(),
                transaction: Some(TransactionHash::Evm(receipt.transaction_hash.0)),
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
                payer: payment.from.into(),
                transaction: Some(TransactionHash::Evm(receipt.transaction_hash.0)),
                network: payload.network,
            })
        }
    }

    async fn supported(&self) -> Result<SupportedPaymentKindsResponse, Self::Error> {
        let kinds = vec![SupportedPaymentKind {
            network: self.network(),
            x402_version: X402Version::V1,
            scheme: Scheme::Exact,
            extra: None,
        }];
        Ok(SupportedPaymentKindsResponse { kinds })
    }
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

/// Validates that the current time is within the `validAfter` and `validBefore` bounds.
///
/// Adds a 6-second grace buffer when checking expiration to account for latency.
///
/// # Errors
/// Returns [`FacilitatorLocalError::InvalidTiming`] if the authorization is not yet active or already expired.
/// Returns [`FacilitatorLocalError::ClockError`] if the system clock cannot be read.
#[instrument(skip_all, err)]
fn assert_time(
    payer: MixedAddress,
    valid_after: UnixTimestamp,
    valid_before: UnixTimestamp,
) -> Result<(), FacilitatorLocalError> {
    let now = UnixTimestamp::try_now().map_err(FacilitatorLocalError::ClockError)?;
    if valid_before < now + 6 {
        return Err(FacilitatorLocalError::InvalidTiming(
            payer,
            format!("Expired: now {} > valid_before {}", now + 6, valid_before),
        ));
    }
    if valid_after > now {
        return Err(FacilitatorLocalError::InvalidTiming(
            payer,
            format!("Not active yet: valid_after {valid_after} > now {now}",),
        ));
    }
    Ok(())
}

/// Verifies the EIP-712 signature in the payment payload.
///
/// Recovers the signing address and checks it matches the expected `from` address in the payload.
///
/// # Errors
/// Returns a [`PaymentError::InvalidSignature`] if the signature is malformed or does not match.
#[instrument(skip_all, err)]
fn assert_signature(
    payer: MixedAddress,
    payload: &ExactEvmPayload,
    domain: &Eip712Domain,
) -> Result<(), FacilitatorLocalError> {
    // Verify the signature
    let signature = Signature::from_raw_array(&payload.signature.0)
        .map_err(|e| FacilitatorLocalError::InvalidSignature(payer.clone(), format!("{e}")))?;
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
        .map_err(|e| FacilitatorLocalError::InvalidSignature(payer.clone(), format!("{e}")))?;
    let expected_address = authorization.from.0;
    if recovered_address != expected_address {
        Err(FacilitatorLocalError::InvalidSignature(
            payer.clone(),
            format!(
                "Address mismatch: recovered: {recovered_address} expected: {expected_address}",
            ),
        ))
    } else {
        Ok(())
    }
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
async fn assert_enough_balance(
    usdc_contract: &USDC::USDCInstance<&InnerProvider>,
    sender: &EvmAddress,
    max_amount_required: U256,
) -> Result<(), FacilitatorLocalError> {
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
        .map_err(|e| FacilitatorLocalError::ContractCall(format!("{e:?}")))?;

    if balance < max_amount_required {
        Err(FacilitatorLocalError::InsufficientFunds((*sender).into()))
    } else {
        Ok(())
    }
}

/// Verifies that the declared `value` in the payload is sufficient for the required amount.
///
/// This is a static check (not on-chain) that compares two numbers.
///
/// # Errors
/// Returns [`FacilitatorLocalError::InsufficientValue`] if the payload's value is less than required.
#[instrument(skip_all, err, fields(
    sent = %sent,
    max_amount_required = %max_amount_required
))]
fn assert_enough_value(
    payer: &EvmAddress,
    sent: &U256,
    max_amount_required: &U256,
) -> Result<(), FacilitatorLocalError> {
    if sent < max_amount_required {
        Err(FacilitatorLocalError::InsufficientValue((*payer).into()))
    } else {
        Ok(())
    }
}
