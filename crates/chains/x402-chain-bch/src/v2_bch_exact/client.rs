//! Client-side BCH transaction construction and signing.

use alloy_primitives::U256;
use async_trait::async_trait;
use bip39::Mnemonic;
use hmac::{Hmac, Mac};
use secp256k1::{Message, PublicKey, Scalar, Secp256k1, SecretKey};
use sha2::Sha512;
use x402_types::proto::v2::{ExtensionsJson, ResourceInfo, X402Version2};
use x402_types::proto::{OriginalJson, PaymentRequired};
use x402_types::scheme::X402SchemeId;
use x402_types::scheme::client::{
    PaymentCandidate, PaymentCandidateSigner, X402Error, X402SchemeClient,
};
use x402_types::util::Base64Bytes;

use crate::address::{CashAddr, hash160, p2pkh_script};
use crate::provider::{BchChainProvider, BchUtxo};
use crate::transaction::{
    BCH_SIGHASH_ALL_FORKID, BchPaymentTarget, BchPolicy, BchTransaction, TxInput, TxOutput,
    is_supported_merchant_script, parse_cash_token_nft, payment_target_with_nft, push_data,
    verify_payment,
};
use crate::transaction::{BchNft, TransactionError};
use crate::v2_bch_exact::V2BchExact;
use crate::v2_bch_exact::types::{
    BchExtra, BchTransactionRequest, ExactBchPayload, PaymentPayload, PaymentRequirements,
};

fn requested_nft(
    extra: &BchExtra,
    asset: &str,
    amount: &str,
) -> Result<Option<BchNft>, TransactionError> {
    let Some(token) = &extra.token else {
        return Ok(None);
    };
    if token.category != asset || token.amount != amount {
        return Err(TransactionError::PolicyViolation(
            "CashToken request must match the x402 asset and amount".to_string(),
        ));
    }
    parse_cash_token_nft(
        token.nft.as_ref().map(|nft| nft.capability.as_str()),
        token.nft.as_ref().map(|nft| nft.commitment.as_str()),
    )
}

/// A signer capable of producing standard BCH ECDSA signatures.
pub trait BchSigner: Clone + Send + Sync + 'static {
    fn public_key(&self) -> Vec<u8>;
    fn sign_digest(&self, digest: [u8; 32]) -> Result<Vec<u8>, String>;
}

/// Wallet boundary for external BCH wallets. Key material and HD state remain wallet-owned.
#[async_trait]
pub trait BchWallet: Send + Sync {
    async fn create_payment(&self, request: BchTransactionRequest) -> Result<Vec<u8>, String>;
}

/// A local secp256k1 signer for P2PKH BCH payments.
#[derive(Clone)]
pub struct Secp256k1BchSigner {
    secret_key: SecretKey,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BchWalletAddress {
    pub address: CashAddr,
    pub path: String,
    pub change: u32,
    pub index: u32,
}

#[derive(Debug, Clone, Copy)]
pub struct BchHdDiscoveryOptions {
    pub account_index: u32,
    pub gap_limit: usize,
    pub max_addresses: usize,
}

impl Default for BchHdDiscoveryOptions {
    fn default() -> Self {
        Self {
            account_index: 0,
            gap_limit: 20,
            max_addresses: 1000,
        }
    }
}

pub struct BchDiscoveredAddress {
    pub wallet: BchWalletAddress,
    pub utxos: Vec<BchUtxo>,
}

const CHIPNET_BIP44_COIN_TYPE: u32 = 1;
const HARDENED_INDEX: u32 = 1 << 31;
type HmacSha512 = Hmac<Sha512>;

impl Secp256k1BchSigner {
    /// Create the default chipnet BIP44 signer at `m/44'/1'/0'/0/0`.
    pub fn from_mnemonic(mnemonic: &str, passphrase: Option<&str>) -> Result<Self, String> {
        Self::from_mnemonic_with_path(mnemonic, passphrase, CHIPNET_BIP44_COIN_TYPE, 0, 0, 0)
    }

    /// Create a signer from a BIP39 mnemonic and an explicit BIP44 path.
    pub fn from_mnemonic_with_path(
        mnemonic: &str,
        passphrase: Option<&str>,
        coin_type: u32,
        account_index: u32,
        change_index: u32,
        address_index: u32,
    ) -> Result<Self, String> {
        if coin_type >= HARDENED_INDEX || account_index >= HARDENED_INDEX {
            return Err("BIP44 coin type and account index must fit a hardened index".to_string());
        }
        let mnemonic = Mnemonic::parse(mnemonic)
            .map_err(|error| format!("invalid BIP39 mnemonic: {error}"))?;
        let seed = mnemonic.to_seed(passphrase.unwrap_or_default());
        let (mut secret_key, mut chain_code) = master_key(&seed)?;
        for index in [
            44 | HARDENED_INDEX,
            coin_type | HARDENED_INDEX,
            account_index | HARDENED_INDEX,
            change_index,
            address_index,
        ] {
            (secret_key, chain_code) = derive_child(secret_key, chain_code, index)?;
        }
        Ok(Self { secret_key })
    }

    pub fn address(&self, network: crate::BchChainReference) -> CashAddr {
        CashAddr {
            network,
            hash160: hash160(&self.public_key()),
        }
    }

    pub fn derive_wallet_address(
        mnemonic: &str,
        network: crate::BchChainReference,
        change: u32,
        index: u32,
        account_index: u32,
        passphrase: Option<&str>,
    ) -> Result<BchWalletAddress, String> {
        let coin_type = if network.is_test_network() { 1 } else { 145 };
        let signer = Self::from_mnemonic_with_path(
            mnemonic,
            passphrase,
            coin_type,
            account_index,
            change,
            index,
        )?;
        Ok(BchWalletAddress {
            address: signer.address(network),
            path: format!("m/44'/{coin_type}'/{account_index}'/{change}/{index}"),
            change,
            index,
        })
    }
}

pub async fn discover_bch_hd_wallet_addresses<P: BchChainProvider + Sync>(
    mnemonic: &str,
    network: crate::BchChainReference,
    provider: &P,
    options: BchHdDiscoveryOptions,
) -> Result<Vec<BchDiscoveredAddress>, String> {
    if options.gap_limit == 0 || options.max_addresses < options.gap_limit {
        return Err("invalid BCH HD discovery limits".to_string());
    }
    let mut discovered = Vec::new();
    for change in [0, 1] {
        let mut unused = 0usize;
        for index in 0..options.max_addresses {
            if unused >= options.gap_limit {
                break;
            }
            let index = u32::try_from(index).map_err(|_| "BCH address index overflow")?;
            let wallet = Secp256k1BchSigner::derive_wallet_address(
                mnemonic,
                network,
                change,
                index,
                options.account_index,
                None,
            )?;
            let utxos = provider
                .list_utxos(&wallet.address)
                .await
                .map_err(|error| error.to_string())?;
            if utxos.is_empty() {
                unused += 1;
            } else {
                unused = 0;
            }
            discovered.push(BchDiscoveredAddress { wallet, utxos });
        }
    }
    Ok(discovered)
}

fn transaction_request(
    requirements: &PaymentRequirements,
) -> Result<BchTransactionRequest, X402Error> {
    let network = crate::BchChainReference::try_from(requirements.network.clone())
        .map_err(|error| X402Error::SigningError(error.to_string()))?;
    Ok(BchTransactionRequest {
        network: if network.is_test_network() {
            crate::v2_bch_exact::types::BchTransactionNetwork::Chipnet
        } else {
            crate::v2_bch_exact::types::BchTransactionNetwork::Mainnet
        },
        recipient: requirements.pay_to.clone(),
        amount: requirements.amount.clone(),
        token: requirements.extra.token.clone(),
    })
}

fn hmac_sha512(key: &[u8], message: &[u8]) -> Result<[u8; 64], String> {
    let mut mac = HmacSha512::new_from_slice(key).map_err(|error| error.to_string())?;
    mac.update(message);
    let digest = mac.finalize().into_bytes();
    let mut output = [0u8; 64];
    output.copy_from_slice(&digest);
    Ok(output)
}

fn master_key(seed: &[u8]) -> Result<(SecretKey, [u8; 32]), String> {
    let digest = hmac_sha512(b"Bitcoin seed", seed)?;
    let mut secret_bytes = [0u8; 32];
    secret_bytes.copy_from_slice(&digest[..32]);
    let mut chain_code = [0u8; 32];
    chain_code.copy_from_slice(&digest[32..]);
    let secret_key = SecretKey::from_byte_array(secret_bytes)
        .map_err(|error| format!("invalid BIP32 master key: {error}"))?;
    Ok((secret_key, chain_code))
}

fn derive_child(
    parent_key: SecretKey,
    parent_chain_code: [u8; 32],
    index: u32,
) -> Result<(SecretKey, [u8; 32]), String> {
    let mut message = Vec::with_capacity(37);
    if index >= HARDENED_INDEX {
        message.push(0);
        message.extend_from_slice(&parent_key.secret_bytes());
    } else {
        message.extend_from_slice(
            &PublicKey::from_secret_key(&Secp256k1::new(), &parent_key).serialize(),
        );
    }
    message.extend_from_slice(&index.to_be_bytes());

    let digest = hmac_sha512(&parent_chain_code, &message)?;
    let mut tweak_bytes = [0u8; 32];
    tweak_bytes.copy_from_slice(&digest[..32]);
    let tweak = Scalar::from_be_bytes(tweak_bytes)
        .map_err(|error| format!("invalid BIP32 child tweak: {error}"))?;
    let child_key = parent_key
        .add_tweak(&tweak)
        .map_err(|error| format!("invalid BIP32 child key: {error}"))?;
    let mut child_chain_code = [0u8; 32];
    child_chain_code.copy_from_slice(&digest[32..]);
    Ok((child_key, child_chain_code))
}

impl BchSigner for Secp256k1BchSigner {
    fn public_key(&self) -> Vec<u8> {
        PublicKey::from_secret_key(&Secp256k1::new(), &self.secret_key)
            .serialize()
            .to_vec()
    }

    fn sign_digest(&self, digest: [u8; 32]) -> Result<Vec<u8>, String> {
        let signature = Secp256k1::new().sign_ecdsa(Message::from_digest(digest), &self.secret_key);
        Ok(signature.serialize_der().to_vec())
    }
}

/// Client for the native BCH v2 exact scheme.
#[derive(Clone)]
pub struct V2BchExactClient<S, P> {
    signer: S,
    provider: P,
    policy: BchPolicy,
}

impl<S, P> V2BchExactClient<S, P> {
    pub fn new(signer: S, provider: P) -> Self {
        Self {
            signer,
            provider,
            policy: BchPolicy::default(),
        }
    }

    pub fn with_policy(mut self, policy: BchPolicy) -> Self {
        self.policy = policy;
        self
    }
}

impl<S, P> X402SchemeId for V2BchExactClient<S, P> {
    fn namespace(&self) -> &str {
        V2BchExact.namespace()
    }

    fn scheme(&self) -> &str {
        V2BchExact.scheme()
    }
}

impl<S, P> X402SchemeClient for V2BchExactClient<S, P>
where
    S: BchSigner,
    P: BchChainProvider + Clone + 'static,
{
    fn accept(&self, payment_required: &PaymentRequired) -> Vec<PaymentCandidate> {
        let payment_required = match payment_required {
            PaymentRequired::V2(payment_required) => payment_required,
            PaymentRequired::V1(_) => return Vec::new(),
        };
        payment_required
            .accepts
            .iter()
            .filter_map(|original| {
                let requirements = PaymentRequirements::try_from(original).ok()?;
                let network =
                    crate::BchChainReference::try_from(requirements.network.clone()).ok()?;
                if requirements.scheme.to_string() != "exact"
                    || requirements.extra.payment_flow != "upfront"
                    || !((requirements.asset == "BCH"
                        && requirements.extra.asset_transfer_method == "native")
                        || (requirements.asset != "BCH"
                            && requirements.extra.asset_transfer_method == "cashtoken"))
                    || network != self.provider.chain_id().try_into().ok()?
                {
                    return None;
                }
                let target = payment_target_with_nft(
                    &requirements.asset,
                    &requirements.amount,
                    &requirements.extra.asset_transfer_method,
                    requirements.extra.token_output_value.as_deref(),
                    requested_nft(
                        &requirements.extra,
                        &requirements.asset,
                        &requirements.amount,
                    )
                    .ok()?,
                    self.policy,
                )
                .ok()?;
                let pay_to = CashAddr::decode_script(&requirements.pay_to, network).ok()?;
                if matches!(&target, BchPaymentTarget::CashToken { .. }) && !pay_to.token_support {
                    return None;
                }
                let amount = match target {
                    BchPaymentTarget::Native { amount, .. }
                    | BchPaymentTarget::CashToken { amount, .. } => amount,
                };
                Some(PaymentCandidate {
                    chain_id: requirements.network.clone(),
                    asset: requirements.asset.clone(),
                    amount: U256::from_limbs([amount, 0, 0, 0]),
                    scheme: self.scheme().to_string(),
                    x402_version: self.x402_version(),
                    pay_to: requirements.pay_to.clone(),
                    signer: Box::new(BchPayloadSigner {
                        signer: self.signer.clone(),
                        provider: self.provider.clone(),
                        policy: self.policy,
                        resource: payment_required.resource.clone(),
                        extensions: payment_required.extensions.clone(),
                        requirements,
                        requirements_json: original.clone(),
                    }),
                })
            })
            .collect()
    }
}

/// Wallet-backed BCH exact client. The wallet owns HD derivation, UTXO selection,
/// change allocation, and signing; x402 only supplies the transaction request.
#[derive(Clone)]
pub struct V2BchExactWalletClient<W, P> {
    wallet: W,
    provider: P,
    policy: BchPolicy,
}

impl<W, P> V2BchExactWalletClient<W, P> {
    pub fn new(wallet: W, provider: P) -> Self {
        Self {
            wallet,
            provider,
            policy: BchPolicy::default(),
        }
    }

    pub fn with_policy(mut self, policy: BchPolicy) -> Self {
        self.policy = policy;
        self
    }
}

impl<W, P> X402SchemeId for V2BchExactWalletClient<W, P> {
    fn namespace(&self) -> &str {
        V2BchExact.namespace()
    }

    fn scheme(&self) -> &str {
        V2BchExact.scheme()
    }
}

impl<W, P> X402SchemeClient for V2BchExactWalletClient<W, P>
where
    W: BchWallet + Clone + 'static,
    P: BchChainProvider + Clone + 'static,
{
    fn accept(&self, payment_required: &PaymentRequired) -> Vec<PaymentCandidate> {
        let payment_required = match payment_required {
            PaymentRequired::V2(payment_required) => payment_required,
            PaymentRequired::V1(_) => return Vec::new(),
        };
        payment_required
            .accepts
            .iter()
            .filter_map(|original| {
                let requirements = PaymentRequirements::try_from(original).ok()?;
                let network =
                    crate::BchChainReference::try_from(requirements.network.clone()).ok()?;
                if requirements.scheme.to_string() != "exact"
                    || requirements.extra.payment_flow != "upfront"
                    || !((requirements.asset == "BCH"
                        && requirements.extra.asset_transfer_method == "native")
                        || (requirements.asset != "BCH"
                            && requirements.extra.asset_transfer_method == "cashtoken"))
                    || network != self.provider.chain_id().try_into().ok()?
                {
                    return None;
                }
                let target = payment_target_with_nft(
                    &requirements.asset,
                    &requirements.amount,
                    &requirements.extra.asset_transfer_method,
                    requirements.extra.token_output_value.as_deref(),
                    requested_nft(
                        &requirements.extra,
                        &requirements.asset,
                        &requirements.amount,
                    )
                    .ok()?,
                    self.policy,
                )
                .ok()?;
                let pay_to = CashAddr::decode_script(&requirements.pay_to, network).ok()?;
                if matches!(&target, BchPaymentTarget::CashToken { .. }) && !pay_to.token_support {
                    return None;
                }
                let amount = match target {
                    BchPaymentTarget::Native { amount, .. }
                    | BchPaymentTarget::CashToken { amount, .. } => amount,
                };
                Some(PaymentCandidate {
                    chain_id: requirements.network.clone(),
                    asset: requirements.asset.clone(),
                    amount: U256::from_limbs([amount, 0, 0, 0]),
                    scheme: self.scheme().to_string(),
                    x402_version: self.x402_version(),
                    pay_to: requirements.pay_to.clone(),
                    signer: Box::new(BchWalletPayloadSigner {
                        wallet: self.wallet.clone(),
                        provider: self.provider.clone(),
                        policy: self.policy,
                        requirements,
                        requirements_json: original.clone(),
                    }),
                })
            })
            .collect()
    }
}

struct BchWalletPayloadSigner<W, P> {
    wallet: W,
    provider: P,
    policy: BchPolicy,
    requirements: PaymentRequirements,
    requirements_json: OriginalJson,
}

#[async_trait]
impl<W, P> PaymentCandidateSigner for BchWalletPayloadSigner<W, P>
where
    W: BchWallet,
    P: BchChainProvider + Sync + 'static,
{
    async fn sign_payment(&self) -> Result<String, X402Error> {
        let request = transaction_request(&self.requirements)?;
        let raw = self
            .wallet
            .create_payment(request)
            .await
            .map_err(X402Error::SigningError)?;
        let transaction = BchTransaction::parse(&raw)
            .map_err(|error| X402Error::SigningError(error.to_string()))?;
        let network = crate::BchChainReference::try_from(self.requirements.network.clone())
            .map_err(|error| X402Error::SigningError(error.to_string()))?;
        let merchant = CashAddr::decode_script(&self.requirements.pay_to, network)
            .map_err(|error| X402Error::SigningError(error.to_string()))?;
        let nft = requested_nft(
            &self.requirements.extra,
            &self.requirements.asset,
            &self.requirements.amount,
        )
        .map_err(|error| X402Error::SigningError(error.to_string()))?;
        let target = payment_target_with_nft(
            &self.requirements.asset,
            &self.requirements.amount,
            &self.requirements.extra.asset_transfer_method,
            self.requirements.extra.token_output_value.as_deref(),
            nft,
            self.policy,
        )
        .map_err(|error| X402Error::SigningError(error.to_string()))?;
        let mut sources = Vec::with_capacity(transaction.inputs.len());
        for input in &transaction.inputs {
            sources.push(
                self.provider
                    .source_output(&input.outpoint)
                    .await
                    .map_err(|error| X402Error::SigningError(error.to_string()))?,
            );
        }
        verify_payment(
            &transaction,
            &sources,
            network,
            &merchant.locking_script(),
            &target,
            self.policy,
        )
        .map_err(|error| X402Error::SigningError(error.to_string()))?;
        let payment = PaymentPayload::<OriginalJson> {
            accepted: self.requirements_json.clone(),
            payload: ExactBchPayload {
                transaction: Base64Bytes::encode(raw).to_string(),
            },
            resource: None,
            x402_version: X402Version2,
            extensions: ExtensionsJson::default(),
        };
        Ok(Base64Bytes::encode(serde_json::to_vec(&payment)?).to_string())
    }
}

struct BchPayloadSigner<S, P> {
    signer: S,
    provider: P,
    policy: BchPolicy,
    resource: Option<ResourceInfo>,
    extensions: ExtensionsJson,
    requirements: PaymentRequirements,
    requirements_json: OriginalJson,
}

#[async_trait]
impl<S, P> PaymentCandidateSigner for BchPayloadSigner<S, P>
where
    S: BchSigner,
    P: BchChainProvider + Sync + 'static,
{
    async fn sign_payment(&self) -> Result<String, X402Error> {
        let network = crate::BchChainReference::try_from(self.requirements.network.clone())
            .map_err(|error| X402Error::SigningError(error.to_string()))?;
        let pay_to = CashAddr::decode_script(&self.requirements.pay_to, network)
            .map_err(|error| X402Error::SigningError(error.to_string()))?;
        let target = payment_target_with_nft(
            &self.requirements.asset,
            &self.requirements.amount,
            &self.requirements.extra.asset_transfer_method,
            self.requirements.extra.token_output_value.as_deref(),
            requested_nft(
                &self.requirements.extra,
                &self.requirements.asset,
                &self.requirements.amount,
            )
            .map_err(|error| X402Error::SigningError(error.to_string()))?,
            self.policy,
        )
        .map_err(|error| X402Error::SigningError(error.to_string()))?;
        if matches!(&target, BchPaymentTarget::CashToken { .. }) && !pay_to.token_support {
            return Err(X402Error::SigningError(
                "CashToken payments require a token-support merchant CashAddr".to_string(),
            ));
        }
        let signer_address = CashAddr {
            network,
            hash160: hash160(&self.signer.public_key()),
        };
        let utxos = self
            .provider
            .list_utxos(&signer_address)
            .await
            .map_err(|error| X402Error::SigningError(error.to_string()))?;
        let mut token_utxos = Vec::new();
        let mut pure_bch_utxos = Vec::new();
        for utxo in utxos {
            if utxo.source_output.script_pubkey != p2pkh_script(&signer_address.hash160) {
                continue;
            }
            match &target {
                BchPaymentTarget::Native { .. } if utxo.source_output.token.is_none() => {
                    pure_bch_utxos.push(utxo)
                }
                BchPaymentTarget::CashToken { category, .. }
                    if utxo.source_output.token.as_ref().is_some_and(|token| {
                        token.category == *category && token.nft.is_none()
                    }) =>
                {
                    token_utxos.push(utxo)
                }
                _ => {}
            }
        }
        token_utxos.sort_by_key(|utxo| std::cmp::Reverse(utxo.source_output.value));
        pure_bch_utxos.sort_by_key(|utxo| std::cmp::Reverse(utxo.source_output.value));

        let candidates = match target {
            BchPaymentTarget::Native { .. } => pure_bch_utxos.clone(),
            BchPaymentTarget::CashToken { .. } => token_utxos.clone(),
        };
        let mut selected = Vec::new();
        let mut selected_value = 0u64;
        let mut selected_token_amount = 0u64;
        for utxo in candidates {
            selected_value = selected_value
                .checked_add(utxo.source_output.value)
                .ok_or_else(|| X402Error::SigningError("UTXO value overflow".to_string()))?;
            selected_token_amount = selected_token_amount
                .checked_add(
                    utxo.source_output
                        .token
                        .as_ref()
                        .map_or(0, |token| token.amount),
                )
                .ok_or_else(|| X402Error::SigningError("CashToken amount overflow".to_string()))?;
            selected.push(utxo);
            let estimated_size = 10usize
                .saturating_add(selected.len().saturating_mul(180))
                .saturating_add(34 * 2);
            let required = target_merchant_value(&target)
                .checked_add(estimated_size as u64 * self.policy.fee_rate_sat_per_byte)
                .ok_or_else(|| X402Error::SigningError("payment amount overflow".to_string()))?;
            if selected_token_covers(&target, selected_token_amount) && selected_value >= required {
                break;
            }
        }
        if matches!(&target, BchPaymentTarget::CashToken { .. }) {
            for utxo in pure_bch_utxos {
                if selected_token_covers(&target, selected_token_amount)
                    && selected_value >= target_merchant_value(&target)
                {
                    break;
                }
                selected_value = selected_value
                    .checked_add(utxo.source_output.value)
                    .ok_or_else(|| X402Error::SigningError("UTXO value overflow".to_string()))?;
                selected.push(utxo);
            }
        }
        if !selected_token_covers(&target, selected_token_amount)
            || selected_value < target_merchant_value(&target)
        {
            return Err(X402Error::SigningError(
                "insufficient BCH/CashToken UTXOs for payment and fee".to_string(),
            ));
        }

        let transaction = build_and_sign_transaction(
            &selected,
            pay_to.locking_script(),
            target,
            &self.signer,
            network,
            self.policy,
        )
        .map_err(|error| X402Error::SigningError(error.to_string()))?;
        let payload = PaymentPayload::<OriginalJson> {
            accepted: self.requirements_json.clone(),
            payload: ExactBchPayload {
                transaction: Base64Bytes::encode(transaction.serialize()).to_string(),
            },
            resource: self.resource.clone(),
            x402_version: X402Version2,
            extensions: self.extensions.clone(),
        };
        Ok(Base64Bytes::encode(serde_json::to_vec(&payload)?).to_string())
    }
}

pub fn build_and_sign_transaction<S: BchSigner>(
    selected: &[BchUtxo],
    merchant_script: Vec<u8>,
    target: BchPaymentTarget,
    signer: &S,
    _network: crate::BchChainReference,
    policy: BchPolicy,
) -> Result<BchTransaction, crate::transaction::TransactionError> {
    if selected.is_empty() || selected.len() > policy.max_inputs {
        return Err(crate::transaction::TransactionError::ExcessiveCount);
    }
    if !is_supported_merchant_script(&merchant_script) {
        return Err(crate::transaction::TransactionError::PolicyViolation(
            "BCH exact requires P2PKH, P2SH20, or P2SH32 merchant output".to_string(),
        ));
    }
    let merchant_value = target_merchant_value(&target);
    if merchant_value < policy.dust_threshold {
        return Err(crate::transaction::TransactionError::PolicyViolation(
            "merchant output is below dust".to_string(),
        ));
    }
    let input_value = selected
        .iter()
        .try_fold(0u64, |total, utxo| {
            total.checked_add(utxo.source_output.value)
        })
        .ok_or(crate::transaction::TransactionError::ArithmeticOverflow)?;
    let input_token_amount = selected
        .iter()
        .try_fold(0u64, |total, utxo| {
            total.checked_add(
                utxo.source_output
                    .token
                    .as_ref()
                    .map_or(0, |token| token.amount),
            )
        })
        .ok_or(crate::transaction::TransactionError::ArithmeticOverflow)?;
    if !selected_token_covers(&target, input_token_amount) {
        return Err(crate::transaction::TransactionError::PolicyViolation(
            "selected CashToken UTXOs do not cover payment".to_string(),
        ));
    }
    if input_value < merchant_value {
        return Err(crate::transaction::TransactionError::PolicyViolation(
            "selected BCH UTXOs do not cover payment".to_string(),
        ));
    }
    let change_script = p2pkh_script(&hash160(&signer.public_key()));
    let mut change = input_value - merchant_value;

    for _ in 0..32 {
        let token_change = match &target {
            BchPaymentTarget::Native { .. } => 0,
            BchPaymentTarget::CashToken { amount, .. } => input_token_amount - amount,
        };
        let include_change = change >= policy.dust_threshold || token_change > 0;
        if include_change && change < policy.dust_threshold {
            return Err(crate::transaction::TransactionError::PolicyViolation(
                "CashToken change requires a dust-valued BCH change output".to_string(),
            ));
        }
        let mut transaction = unsigned_transaction(
            selected,
            merchant_script.clone(),
            &target,
            if include_change {
                Some((
                    change,
                    change_script.clone(),
                    match &target {
                        BchPaymentTarget::CashToken { category, .. } if token_change > 0 => {
                            Some(crate::transaction::BchToken {
                                category: *category,
                                amount: token_change,
                                nft: None,
                            })
                        }
                        _ => None,
                    },
                ))
            } else {
                None
            },
        );
        sign_transaction(&mut transaction, selected, signer)?;
        let actual_size = transaction.serialize().len() as u64;
        let required_fee = actual_size
            .checked_mul(policy.fee_rate_sat_per_byte)
            .ok_or(crate::transaction::TransactionError::ArithmeticOverflow)?;
        let available = input_value - merchant_value;
        if !include_change {
            if available < required_fee {
                return Err(crate::transaction::TransactionError::PolicyViolation(
                    "selected BCH UTXOs do not cover fee".to_string(),
                ));
            }
            if transaction.serialize().len() > policy.max_transaction_size {
                return Err(crate::transaction::TransactionError::PolicyViolation(
                    "transaction exceeds maximum size".to_string(),
                ));
            }
            return Ok(transaction);
        }

        let desired_change = available
            .checked_sub(required_fee)
            .ok_or(crate::transaction::TransactionError::ArithmeticOverflow)?;
        if desired_change < policy.dust_threshold {
            if token_change > 0 {
                return Err(crate::transaction::TransactionError::PolicyViolation(
                    "selected BCH UTXOs do not cover token change dust".to_string(),
                ));
            }
            change = 0;
            continue;
        }
        if change > desired_change {
            change = desired_change;
            continue;
        }
        if transaction.serialize().len() > policy.max_transaction_size {
            return Err(crate::transaction::TransactionError::PolicyViolation(
                "transaction exceeds maximum size".to_string(),
            ));
        }
        return Ok(transaction);
    }
    Err(crate::transaction::TransactionError::PolicyViolation(
        "fee/change calculation did not converge".to_string(),
    ))
}

fn unsigned_transaction(
    selected: &[BchUtxo],
    merchant_script: Vec<u8>,
    target: &BchPaymentTarget,
    change: Option<(u64, Vec<u8>, Option<crate::transaction::BchToken>)>,
) -> BchTransaction {
    let inputs = selected
        .iter()
        .map(|utxo| TxInput {
            outpoint: utxo.outpoint,
            script_sig: Vec::new(),
            sequence: u32::MAX,
        })
        .collect();
    let mut outputs = vec![TxOutput {
        value: target_merchant_value(target),
        script_pubkey: merchant_script,
        token: match target {
            BchPaymentTarget::CashToken {
                category, amount, ..
            } => Some(crate::transaction::BchToken {
                category: *category,
                amount: *amount,
                nft: None,
            }),
            BchPaymentTarget::Native { .. } => None,
        },
    }];
    if let Some((value, script_pubkey, token)) = change {
        outputs.push(TxOutput {
            value,
            script_pubkey,
            token,
        });
    }
    BchTransaction {
        version: 2,
        inputs,
        outputs,
        lock_time: 0,
    }
}

fn target_merchant_value(target: &BchPaymentTarget) -> u64 {
    match target {
        BchPaymentTarget::Native { merchant_value, .. }
        | BchPaymentTarget::CashToken { merchant_value, .. } => *merchant_value,
    }
}

fn selected_token_covers(target: &BchPaymentTarget, selected_token_amount: u64) -> bool {
    match target {
        BchPaymentTarget::Native { .. } => true,
        BchPaymentTarget::CashToken { amount, .. } => selected_token_amount >= *amount,
    }
}

fn sign_transaction<S: BchSigner>(
    transaction: &mut BchTransaction,
    selected: &[BchUtxo],
    signer: &S,
) -> Result<(), crate::transaction::TransactionError> {
    let public_key = signer.public_key();
    for (index, utxo) in selected.iter().enumerate() {
        let digest =
            transaction.signing_hash(index, &utxo.source_output, BCH_SIGHASH_ALL_FORKID)?;
        let mut signature = signer
            .sign_digest(digest)
            .map_err(|_| crate::transaction::TransactionError::InvalidSignature)?;
        signature.push(BCH_SIGHASH_ALL_FORKID as u8);
        let mut script_sig = push_data(&signature)?;
        script_sig.extend_from_slice(&push_data(&public_key)?);
        transaction.inputs[index].script_sig = script_sig;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIP39_VECTOR_MNEMONIC: &str =
        "legal winner thank year wave sausage worth useful legal winner thank yellow";

    #[test]
    fn derives_the_chipnet_bip44_signer() {
        let signer = Secp256k1BchSigner::from_mnemonic(BIP39_VECTOR_MNEMONIC, None).unwrap();

        assert_eq!(
            signer.address(crate::BchChainReference::Chipnet).encode(),
            "bchtest:qp7zymyamk2cf6rxgdwcagxwzmyaqjg9ksvqydu0dl"
        );
    }

    #[test]
    fn derives_receive_and_change_wallet_paths_for_mainnet_and_chipnet() {
        let receive = Secp256k1BchSigner::derive_wallet_address(
            BIP39_VECTOR_MNEMONIC,
            crate::BchChainReference::Chipnet,
            0,
            0,
            0,
            None,
        )
        .unwrap();
        let change = Secp256k1BchSigner::derive_wallet_address(
            BIP39_VECTOR_MNEMONIC,
            crate::BchChainReference::Chipnet,
            1,
            0,
            0,
            None,
        )
        .unwrap();
        let mainnet = Secp256k1BchSigner::derive_wallet_address(
            BIP39_VECTOR_MNEMONIC,
            crate::BchChainReference::Mainnet,
            0,
            0,
            0,
            None,
        )
        .unwrap();
        assert_eq!(receive.path, "m/44'/1'/0'/0/0");
        assert_eq!(change.path, "m/44'/1'/0'/1/0");
        assert_eq!(mainnet.path, "m/44'/145'/0'/0/0");
        assert_ne!(receive.address, change.address);
        assert_ne!(receive.address, mainnet.address);
    }

    #[test]
    fn rejects_an_invalid_bip39_mnemonic() {
        let error = match Secp256k1BchSigner::from_mnemonic("not a valid mnemonic", None) {
            Ok(_) => panic!("invalid mnemonic was accepted"),
            Err(error) => error,
        };

        assert!(error.starts_with("invalid BIP39 mnemonic:"));
    }
}
