use crate::X402PaymentsError;
use crate::chains::{IntoSenderWallet, SenderWallet};
use alloy::primitives::FixedBytes;
use alloy::signers::Signer;
use alloy::signers::local::PrivateKeySigner;
use alloy::sol_types::{SolStruct, eip712_domain};
use async_trait::async_trait;
use rand::{Rng, rng};
use std::sync::Arc;
use x402_rs::chain::evm::EvmChain;
use x402_rs::network::NetworkFamily;
use x402_rs::timestamp::UnixTimestamp;
use x402_rs::types::{
    EvmSignature, ExactEvmPayload, ExactEvmPayloadAuthorization, ExactPaymentPayload,
    HexEncodedNonce, PaymentPayload, PaymentRequirements, Scheme, TransferWithAuthorization,
};

#[derive(Clone)]
pub struct EvmSenderWallet {
    signer: Arc<dyn Signer + Send + Sync>,
}

impl EvmSenderWallet {
    pub fn new(signer: impl Signer + Send + Sync + 'static) -> Self {
        Self {
            signer: Arc::new(signer),
        }
    }
}

impl<S> From<S> for EvmSenderWallet
where
    S: Signer + Send + Sync + 'static,
{
    fn from(signer: S) -> Self {
        Self::new(signer)
    }
}

impl IntoSenderWallet for PrivateKeySigner {
    fn into_sender_wallet(self) -> Arc<dyn SenderWallet> {
        Arc::new(EvmSenderWallet::new(self))
    }
}

impl IntoSenderWallet for EvmSenderWallet {
    fn into_sender_wallet(self) -> Arc<dyn SenderWallet> {
        Arc::new(self)
    }
}

#[async_trait]
impl SenderWallet for EvmSenderWallet {
    fn can_handle(&self, requirements: &PaymentRequirements) -> bool {
        let network = requirements.network;
        let network_family: NetworkFamily = network.into();
        match network_family {
            NetworkFamily::Evm => true,
            NetworkFamily::Solana => false,
        }
    }

    async fn payment_payload(
        &self,
        selected: PaymentRequirements,
    ) -> Result<PaymentPayload, X402PaymentsError> {
        let (name, version) = match selected.extra {
            None => (None, None),
            Some(extra) => {
                let name = extra
                    .get("name")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned);
                let version = extra
                    .get("version")
                    .and_then(|v| v.as_str())
                    .map(ToOwned::to_owned);
                (name, version)
            }
        };
        let network = selected.network;
        let evm_chain: EvmChain = network
            .try_into()
            .map_err(|e| X402PaymentsError::SigningError(format!("{e:?}")))?;
        let chain_id = evm_chain.chain_id;
        let domain = eip712_domain! {
            name: name.unwrap_or("".to_string()),
            version: version.unwrap_or("".to_string()),
            chain_id: chain_id,
            verifying_contract: selected.asset.try_into().map_err(X402PaymentsError::InvalidEVMAddress)?,
        };
        let now = UnixTimestamp::try_now().map_err(X402PaymentsError::ClockError)?;
        let valid_after = UnixTimestamp(now.seconds_since_epoch() - 10 * 60); // 10 mins before
        let valid_before = now + selected.max_timeout_seconds;
        let nonce: [u8; 32] = rng().random();
        let authorization = ExactEvmPayloadAuthorization {
            from: self.signer.address().into(),
            to: selected
                .pay_to
                .try_into()
                .map_err(X402PaymentsError::InvalidEVMAddress)?,
            value: selected.max_amount_required,
            valid_after,
            valid_before,
            nonce: HexEncodedNonce(nonce),
        };
        #[cfg(feature = "telemetry")]
        tracing::debug!(?authorization, "Constructed authorization payload");
        let transfer_with_authorization = TransferWithAuthorization {
            from: authorization.from.into(),
            to: authorization.to.into(),
            value: authorization.value.into(),
            validAfter: authorization.valid_after.into(),
            validBefore: authorization.valid_before.into(),
            nonce: FixedBytes(nonce),
        };
        let eip712_hash = transfer_with_authorization.eip712_signing_hash(&domain);
        let signature = self
            .signer
            .sign_hash(&eip712_hash)
            .await
            .map_err(|e| X402PaymentsError::SigningError(format!("{e:?}")))?;
        #[cfg(feature = "telemetry")]
        tracing::debug!(?signature, "Signature obtained");
        let payment_payload = PaymentPayload {
            x402_version: x402_rs::types::X402Version::V1,
            scheme: Scheme::Exact,
            network,
            payload: ExactPaymentPayload::Evm(ExactEvmPayload {
                signature: EvmSignature::from(signature.as_bytes()),
                authorization,
            }),
        };
        Ok(payment_payload)
    }
}
