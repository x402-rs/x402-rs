//! Bitcoin Cash transaction parsing, serialization, and P2PKH validation.

use secp256k1::{Message, PublicKey, Secp256k1, ecdsa::Signature};
use sha2::{Digest, Sha256};
use std::fmt::{Display, Formatter};

use crate::address::{CashAddr, hash160, p2pkh_script};
use crate::chain::BchChainReference;

pub const BCH_SIGHASH_ALL_FORKID: u32 = 0x41;

pub fn parse_canonical_satoshi_amount(value: &str) -> Result<u64, TransactionError> {
    if value.is_empty()
        || (value != "0" && value.starts_with('0'))
        || !value.bytes().all(|byte| byte.is_ascii_digit())
    {
        return Err(TransactionError::PolicyViolation(
            "BCH amount must be canonical satoshis".to_string(),
        ));
    }
    value
        .parse::<u64>()
        .map_err(|_| TransactionError::ArithmeticOverflow)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TxId(pub [u8; 32]);

impl TxId {
    pub fn from_hex(value: &str) -> Result<Self, TransactionError> {
        let bytes = hex::decode(value).map_err(|_| TransactionError::InvalidHex)?;
        if bytes.len() != 32 {
            return Err(TransactionError::InvalidHex);
        }
        let mut txid = [0u8; 32];
        txid.copy_from_slice(&bytes);
        Ok(Self(txid))
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl Display for TxId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", hex::encode(self.0))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OutPoint {
    pub txid: TxId,
    pub vout: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxInput {
    pub outpoint: OutPoint,
    pub script_sig: Vec<u8>,
    pub sequence: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TxOutput {
    pub value: u64,
    pub script_pubkey: Vec<u8>,
    pub token: Option<BchToken>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BchTransaction {
    pub version: i32,
    pub inputs: Vec<TxInput>,
    pub outputs: Vec<TxOutput>,
    pub lock_time: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceOutput {
    pub value: u64,
    pub script_pubkey: Vec<u8>,
    pub token: Option<BchToken>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BchTokenCapability {
    None,
    Mutable,
    Minting,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BchNft {
    pub capability: BchTokenCapability,
    pub commitment: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BchToken {
    /// CashToken category in user-interface byte order.
    pub category: [u8; 32],
    pub amount: u64,
    pub nft: Option<BchNft>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BchPaymentTarget {
    Native {
        amount: u64,
        merchant_value: u64,
    },
    CashToken {
        category: [u8; 32],
        amount: u64,
        merchant_value: u64,
        nft: Option<BchNft>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TransactionError {
    #[error("transaction is truncated")]
    Truncated,
    #[error("transaction contains a non-canonical compact-size integer")]
    NonCanonicalVarInt,
    #[error("transaction contains an unsupported witness marker")]
    WitnessUnsupported,
    #[error("transaction contains too many inputs or outputs")]
    ExcessiveCount,
    #[error("transaction contains an invalid script")]
    InvalidScript,
    #[error("transaction contains an invalid signature")]
    InvalidSignature,
    #[error("transaction contains an unsupported sighash type")]
    UnsupportedSighash,
    #[error("transaction contains an invalid public key")]
    InvalidPublicKey,
    #[error("transaction contains an invalid hex value")]
    InvalidHex,
    #[error("transaction arithmetic overflowed")]
    ArithmeticOverflow,
    #[error("transaction has an invalid input or output policy")]
    PolicyViolation(String),
}

impl BchTransaction {
    pub fn parse(raw: &[u8]) -> Result<Self, TransactionError> {
        let mut reader = Reader::new(raw);
        let version = reader.i32()?;
        let input_count = reader.varint()? as usize;
        if input_count == 0 || input_count > 10_000 {
            return Err(TransactionError::ExcessiveCount);
        }
        let mut inputs = Vec::with_capacity(input_count);
        for _ in 0..input_count {
            let mut txid_wire = [0u8; 32];
            txid_wire.copy_from_slice(reader.take(32)?);
            txid_wire.reverse();
            let vout = reader.u32()?;
            let script_sig = reader.bytes()?;
            let sequence = reader.u32()?;
            inputs.push(TxInput {
                outpoint: OutPoint {
                    txid: TxId(txid_wire),
                    vout,
                },
                script_sig,
                sequence,
            });
        }
        let output_count = reader.varint()? as usize;
        if output_count == 0 || output_count > 10_000 {
            return Err(TransactionError::ExcessiveCount);
        }
        let mut outputs = Vec::with_capacity(output_count);
        for _ in 0..output_count {
            let value = reader.u64()?;
            let field = reader.bytes()?;
            let (token, script_pubkey) = parse_token_prefix_and_script(&field)?;
            outputs.push(TxOutput {
                value,
                script_pubkey,
                token,
            });
        }
        let lock_time = reader.u32()?;
        if !reader.is_empty() {
            return Err(TransactionError::PolicyViolation(
                "trailing bytes after locktime".to_string(),
            ));
        }
        Ok(Self {
            version,
            inputs,
            outputs,
            lock_time,
        })
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut result = Vec::new();
        result.extend_from_slice(&self.version.to_le_bytes());
        write_varint(self.inputs.len() as u64, &mut result);
        for input in &self.inputs {
            let mut txid_wire = input.outpoint.txid.0;
            txid_wire.reverse();
            result.extend_from_slice(&txid_wire);
            result.extend_from_slice(&input.outpoint.vout.to_le_bytes());
            write_bytes(&input.script_sig, &mut result);
            result.extend_from_slice(&input.sequence.to_le_bytes());
        }
        write_varint(self.outputs.len() as u64, &mut result);
        for output in &self.outputs {
            result.extend_from_slice(&output.value.to_le_bytes());
            let field =
                serialize_token_prefix_and_script(output.token.as_ref(), &output.script_pubkey)
                    .expect("BCH transaction output token data must be valid");
            write_bytes(&field, &mut result);
        }
        result.extend_from_slice(&self.lock_time.to_le_bytes());
        result
    }

    pub fn txid(&self) -> TxId {
        let digest = double_sha256(&self.serialize());
        let mut txid = digest;
        txid.reverse();
        TxId(txid)
    }

    pub fn signing_hash(
        &self,
        input_index: usize,
        source_output: &SourceOutput,
        sighash_type: u32,
    ) -> Result<[u8; 32], TransactionError> {
        if sighash_type != BCH_SIGHASH_ALL_FORKID {
            return Err(TransactionError::UnsupportedSighash);
        }
        let input = self
            .inputs
            .get(input_index)
            .ok_or(TransactionError::Truncated)?;
        let mut preimage = Vec::new();
        preimage.extend_from_slice(&self.version.to_le_bytes());

        let mut prevouts = Vec::with_capacity(self.inputs.len() * 36);
        for candidate in &self.inputs {
            let mut txid_wire = candidate.outpoint.txid.0;
            txid_wire.reverse();
            prevouts.extend_from_slice(&txid_wire);
            prevouts.extend_from_slice(&candidate.outpoint.vout.to_le_bytes());
        }
        preimage.extend_from_slice(&double_sha256(&prevouts));

        let mut sequences = Vec::with_capacity(self.inputs.len() * 4);
        for candidate in &self.inputs {
            sequences.extend_from_slice(&candidate.sequence.to_le_bytes());
        }
        preimage.extend_from_slice(&double_sha256(&sequences));

        let mut txid_wire = input.outpoint.txid.0;
        txid_wire.reverse();
        preimage.extend_from_slice(&txid_wire);
        preimage.extend_from_slice(&input.outpoint.vout.to_le_bytes());
        let source_token_prefix = serialize_token_prefix(source_output.token.as_ref())
            .map_err(|_| TransactionError::InvalidScript)?;
        preimage.extend_from_slice(&source_token_prefix);
        write_bytes(&source_output.script_pubkey, &mut preimage);
        preimage.extend_from_slice(&source_output.value.to_le_bytes());
        preimage.extend_from_slice(&input.sequence.to_le_bytes());

        let mut outputs = Vec::new();
        for output in &self.outputs {
            outputs.extend_from_slice(&output.value.to_le_bytes());
            let field =
                serialize_token_prefix_and_script(output.token.as_ref(), &output.script_pubkey)
                    .map_err(|_| TransactionError::InvalidScript)?;
            write_bytes(&field, &mut outputs);
        }
        preimage.extend_from_slice(&double_sha256(&outputs));
        preimage.extend_from_slice(&self.lock_time.to_le_bytes());
        preimage.extend_from_slice(&sighash_type.to_le_bytes());
        Ok(double_sha256(&preimage))
    }

    pub fn verify_p2pkh_input(
        &self,
        input_index: usize,
        source_output: &SourceOutput,
    ) -> Result<[u8; 20], TransactionError> {
        let input = self
            .inputs
            .get(input_index)
            .ok_or(TransactionError::Truncated)?;
        if !is_p2pkh_script(&source_output.script_pubkey) {
            return Err(TransactionError::PolicyViolation(
                "source output is not P2PKH".to_string(),
            ));
        }
        let pushes = parse_pushes(&input.script_sig)?;
        if pushes.len() != 2 || pushes[0].len() < 2 {
            return Err(TransactionError::InvalidScript);
        }
        let signature_bytes = pushes[0];
        let sighash_type = *signature_bytes
            .last()
            .ok_or(TransactionError::InvalidSignature)?;
        if u32::from(sighash_type) != BCH_SIGHASH_ALL_FORKID {
            return Err(TransactionError::UnsupportedSighash);
        }
        let signature = Signature::from_der(&signature_bytes[..signature_bytes.len() - 1])
            .map_err(|_| TransactionError::InvalidSignature)?;
        let public_key =
            PublicKey::from_slice(pushes[1]).map_err(|_| TransactionError::InvalidPublicKey)?;
        let public_key_bytes = public_key.serialize();
        let public_key_hash = hash160(&public_key_bytes);
        if source_output.script_pubkey[3..23] != public_key_hash[..] {
            return Err(TransactionError::InvalidSignature);
        }
        let digest = self.signing_hash(input_index, source_output, BCH_SIGHASH_ALL_FORKID)?;
        let message = Message::from_digest(digest);
        Secp256k1::verification_only()
            .verify_ecdsa(message, &signature, &public_key)
            .map_err(|_| TransactionError::InvalidSignature)?;
        Ok(public_key_hash)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BchPolicy {
    pub fee_rate_sat_per_byte: u64,
    pub dust_threshold: u64,
    pub max_transaction_size: usize,
    pub max_inputs: usize,
}

impl Default for BchPolicy {
    fn default() -> Self {
        Self {
            fee_rate_sat_per_byte: 1,
            dust_threshold: 546,
            max_transaction_size: 100_000,
            max_inputs: 100,
        }
    }
}

pub fn parse_cash_token_category(value: &str) -> Result<[u8; 32], TransactionError> {
    if value != value.to_ascii_lowercase()
        || value.len() != 64
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(TransactionError::InvalidHex);
    }
    let bytes = hex::decode(value).map_err(|_| TransactionError::InvalidHex)?;
    if bytes.len() != 32 {
        return Err(TransactionError::PolicyViolation(
            "CashToken category must be 32 bytes".to_string(),
        ));
    }
    let mut category = [0u8; 32];
    category.copy_from_slice(&bytes);
    Ok(category)
}

pub fn payment_target(
    asset: &str,
    amount: &str,
    asset_transfer_method: &str,
    token_output_value: Option<&str>,
    policy: BchPolicy,
) -> Result<BchPaymentTarget, TransactionError> {
    payment_target_with_nft(
        asset,
        amount,
        asset_transfer_method,
        token_output_value,
        None,
        policy,
    )
}

pub fn payment_target_with_nft(
    asset: &str,
    amount: &str,
    asset_transfer_method: &str,
    token_output_value: Option<&str>,
    nft: Option<BchNft>,
    policy: BchPolicy,
) -> Result<BchPaymentTarget, TransactionError> {
    let amount = parse_canonical_satoshi_amount(amount)?;
    if asset == "BCH" {
        if asset_transfer_method != "native" {
            return Err(TransactionError::PolicyViolation(
                "BCH requires native asset transfer method".to_string(),
            ));
        }
        return Ok(BchPaymentTarget::Native {
            amount,
            merchant_value: amount,
        });
    }
    if asset_transfer_method != "cashtoken" {
        return Err(TransactionError::PolicyViolation(
            "CashToken requires the cashtoken asset transfer method".to_string(),
        ));
    }
    if (amount == 0 && nft.is_none()) || amount > i64::MAX as u64 {
        return Err(TransactionError::PolicyViolation(
            "CashToken amount is outside the BCH token range".to_string(),
        ));
    }
    let merchant_value = token_output_value
        .map(parse_canonical_satoshi_amount)
        .transpose()?
        .unwrap_or(policy.dust_threshold);
    Ok(BchPaymentTarget::CashToken {
        category: parse_cash_token_category(asset)?,
        amount,
        merchant_value,
        nft,
    })
}

pub fn parse_cash_token_nft(
    capability: Option<&str>,
    commitment: Option<&str>,
) -> Result<Option<BchNft>, TransactionError> {
    match (capability, commitment) {
        (None, None) => Ok(None),
        (Some(capability), Some(commitment)) => {
            let capability = match capability {
                "none" => BchTokenCapability::None,
                "mutable" => BchTokenCapability::Mutable,
                "minting" => BchTokenCapability::Minting,
                _ => {
                    return Err(TransactionError::PolicyViolation(
                        "invalid CashToken NFT capability".to_string(),
                    ));
                }
            };
            let commitment = hex::decode(commitment).map_err(|_| TransactionError::InvalidHex)?;
            if commitment.is_empty() || commitment.len() > 40 {
                return Err(TransactionError::PolicyViolation(
                    "invalid CashToken NFT commitment".to_string(),
                ));
            }
            Ok(Some(BchNft {
                capability,
                commitment,
            }))
        }
        _ => Err(TransactionError::PolicyViolation(
            "CashToken NFT capability and commitment must be provided together".to_string(),
        )),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedPayment {
    pub txid: TxId,
    pub payer: CashAddr,
    pub fee: u64,
    pub input_value: u64,
    pub output_value: u64,
}

pub fn verify_payment(
    transaction: &BchTransaction,
    source_outputs: &[SourceOutput],
    network: BchChainReference,
    merchant_script: &[u8],
    target: &BchPaymentTarget,
    policy: BchPolicy,
) -> Result<VerifiedPayment, TransactionError> {
    validate_payment_target(target)?;
    let serialized_size = transaction.serialize().len();
    if serialized_size > policy.max_transaction_size {
        return Err(TransactionError::PolicyViolation(
            "transaction exceeds maximum size".to_string(),
        ));
    }
    if transaction.inputs.is_empty() || transaction.inputs.len() > policy.max_inputs {
        return Err(TransactionError::ExcessiveCount);
    }
    if transaction.lock_time != 0 {
        return Err(TransactionError::PolicyViolation(
            "non-zero locktime is not supported by BCH exact".to_string(),
        ));
    }
    let merchant_value = match target {
        BchPaymentTarget::Native { merchant_value, .. }
        | BchPaymentTarget::CashToken { merchant_value, .. } => *merchant_value,
    };
    if merchant_value < policy.dust_threshold {
        return Err(TransactionError::PolicyViolation(
            "merchant output is below dust".to_string(),
        ));
    }
    if source_outputs.len() != transaction.inputs.len() {
        return Err(TransactionError::PolicyViolation(
            "source output count does not match transaction inputs".to_string(),
        ));
    }
    if transaction.outputs.len() > 2 {
        return Err(TransactionError::PolicyViolation(
            "BCH exact permits one merchant output and one change output".to_string(),
        ));
    }

    let mut input_value = 0u64;
    let mut input_token_amount = 0u64;
    let mut input_nft: Option<BchNft> = None;
    let mut payer_hash = None;
    for (index, source_output) in source_outputs.iter().enumerate() {
        input_value = input_value
            .checked_add(source_output.value)
            .ok_or(TransactionError::ArithmeticOverflow)?;
        let hash = transaction.verify_p2pkh_input(index, source_output)?;
        if let Some(existing) = payer_hash {
            if existing != hash {
                return Err(TransactionError::PolicyViolation(
                    "all BCH inputs must belong to the same payer".to_string(),
                ));
            }
        } else {
            payer_hash = Some(hash);
        }
        match target {
            BchPaymentTarget::Native { .. } => {
                if source_output.token.is_some() {
                    return Err(TransactionError::PolicyViolation(
                        "native BCH payment cannot spend CashTokens".to_string(),
                    ));
                }
            }
            BchPaymentTarget::CashToken { category, nft, .. } => {
                if let Some(token) = &source_output.token {
                    if &token.category != category
                        || (nft.is_none() && token.nft.is_some())
                        || (nft.is_some()
                            && token
                                .nft
                                .as_ref()
                                .is_some_and(|actual| Some(actual) != nft.as_ref()))
                    {
                        return Err(TransactionError::PolicyViolation(
                            "CashToken input does not match the requested category or NFT"
                                .to_string(),
                        ));
                    }
                    if token.nft.is_some() {
                        if input_nft.is_some() {
                            return Err(TransactionError::PolicyViolation(
                                "multiple NFT inputs are not supported".to_string(),
                            ));
                        }
                        input_nft = token.nft.clone();
                    }
                    input_token_amount = input_token_amount
                        .checked_add(token.amount)
                        .ok_or(TransactionError::ArithmeticOverflow)?;
                }
            }
        }
    }

    let requested_token_amount = match target {
        BchPaymentTarget::Native { .. } => 0,
        BchPaymentTarget::CashToken { amount, .. } => {
            if input_token_amount < *amount {
                return Err(TransactionError::PolicyViolation(
                    "CashToken inputs do not cover the requested amount".to_string(),
                ));
            }
            *amount
        }
    };
    if let BchPaymentTarget::CashToken {
        nft: Some(expected),
        ..
    } = target
        && input_nft.as_ref() != Some(expected)
    {
        return Err(TransactionError::PolicyViolation(
            "CashToken inputs do not contain the requested NFT".to_string(),
        ));
    }

    let merchant_matches = transaction
        .outputs
        .iter()
        .filter(|output| {
            output.value == merchant_value
                && output.script_pubkey == merchant_script
                && merchant_token_matches(output.token.as_ref(), target)
        })
        .count();
    if merchant_matches != 1 {
        return Err(TransactionError::PolicyViolation(
            "transaction must contain exactly one exact merchant output".to_string(),
        ));
    }

    let mut output_value = 0u64;
    let mut output_token_amount = 0u64;
    for output in &transaction.outputs {
        let is_merchant = output.value == merchant_value && output.script_pubkey == merchant_script;
        if !is_merchant && !is_p2pkh_script(&output.script_pubkey) {
            return Err(TransactionError::PolicyViolation(
                "BCH exact change outputs must be standard P2PKH".to_string(),
            ));
        }
        if let Some(token) = &output.token {
            match target {
                BchPaymentTarget::Native { .. } => {
                    return Err(TransactionError::PolicyViolation(
                        "native BCH payment cannot create CashTokens".to_string(),
                    ));
                }
                BchPaymentTarget::CashToken { category, nft, .. }
                    if &token.category != category
                        || (nft.is_none() && token.nft.is_some())
                        || (nft.is_some()
                            && token
                                .nft
                                .as_ref()
                                .is_some_and(|actual| Some(actual) != nft.as_ref())) =>
                {
                    return Err(TransactionError::PolicyViolation(
                        "CashToken output category does not match the payment".to_string(),
                    ));
                }
                BchPaymentTarget::CashToken { .. } => {}
            }
            output_token_amount = output_token_amount
                .checked_add(token.amount)
                .ok_or(TransactionError::ArithmeticOverflow)?;
        }
        output_value = output_value
            .checked_add(output.value)
            .ok_or(TransactionError::ArithmeticOverflow)?;
    }
    if output_token_amount != input_token_amount {
        return Err(TransactionError::PolicyViolation(
            "CashToken amount is not conserved".to_string(),
        ));
    }
    if requested_token_amount == 0 && output_token_amount != 0 {
        return Err(TransactionError::PolicyViolation(
            "native BCH payment cannot contain CashTokens".to_string(),
        ));
    }
    let fee = input_value
        .checked_sub(output_value)
        .ok_or(TransactionError::PolicyViolation(
            "outputs exceed input value".to_string(),
        ))?;
    let minimum_fee = (serialized_size as u64)
        .checked_mul(policy.fee_rate_sat_per_byte)
        .ok_or(TransactionError::ArithmeticOverflow)?;
    if fee < minimum_fee {
        return Err(TransactionError::PolicyViolation(
            "transaction fee is below the BCH exact minimum".to_string(),
        ));
    }
    if transaction.outputs.len() == 2 {
        let change = transaction
            .outputs
            .iter()
            .find(|output| {
                !(output.value == merchant_value && output.script_pubkey == merchant_script)
            })
            .ok_or_else(|| {
                TransactionError::PolicyViolation("missing change output".to_string())
            })?;
        if change.value < policy.dust_threshold {
            return Err(TransactionError::PolicyViolation(
                "change output is below dust threshold".to_string(),
            ));
        }
        if change.script_pubkey == merchant_script {
            return Err(TransactionError::PolicyViolation(
                "duplicate merchant output is not valid change".to_string(),
            ));
        }
        if !payer_hash
            .map(|hash| change.script_pubkey == p2pkh_script(&hash))
            .unwrap_or(false)
        {
            return Err(TransactionError::PolicyViolation(
                "change output must return to the payer".to_string(),
            ));
        }
        match target {
            BchPaymentTarget::Native { .. } => {
                if change.token.is_some() {
                    return Err(TransactionError::PolicyViolation(
                        "native BCH change cannot contain CashTokens".to_string(),
                    ));
                }
            }
            BchPaymentTarget::CashToken { category, .. } => {
                let expected_change = input_token_amount - requested_token_amount;
                match (expected_change, &change.token) {
                    (0, None) => {}
                    (amount, Some(token))
                        if amount == token.amount
                            && token.category == *category
                            && token.nft.is_none() => {}
                    _ => {
                        return Err(TransactionError::PolicyViolation(
                            "CashToken change does not return the exact remainder to the payer"
                                .to_string(),
                        ));
                    }
                }
            }
        }
    } else if requested_token_amount != input_token_amount {
        return Err(TransactionError::PolicyViolation(
            "CashToken remainder is missing".to_string(),
        ));
    }

    let payer_hash = payer_hash.ok_or(TransactionError::InvalidSignature)?;
    Ok(VerifiedPayment {
        txid: transaction.txid(),
        payer: CashAddr {
            network,
            hash160: payer_hash,
        },
        fee,
        input_value,
        output_value,
    })
}

fn validate_payment_target(target: &BchPaymentTarget) -> Result<(), TransactionError> {
    if let BchPaymentTarget::CashToken { amount, .. } = target
        && (*amount == 0 || *amount > i64::MAX as u64)
    {
        return Err(TransactionError::PolicyViolation(
            "CashToken amount is outside the BCH token range".to_string(),
        ));
    }
    Ok(())
}

fn merchant_token_matches(token: Option<&BchToken>, target: &BchPaymentTarget) -> bool {
    match target {
        BchPaymentTarget::Native { .. } => token.is_none(),
        BchPaymentTarget::CashToken {
            category,
            amount,
            nft,
            ..
        } => token.is_some_and(|token| {
            token.category == *category && token.amount == *amount && token.nft == *nft
        }),
    }
}

pub fn is_p2pkh_script(script: &[u8]) -> bool {
    script.len() == 25
        && script[0] == 0x76
        && script[1] == 0xa9
        && script[2] == 0x14
        && script[23] == 0x88
        && script[24] == 0xac
}

pub fn is_p2sh20_script(script: &[u8]) -> bool {
    script.len() == 23 && script[0] == 0xa9 && script[1] == 0x14 && script[22] == 0x87
}

pub fn is_p2sh32_script(script: &[u8]) -> bool {
    script.len() == 35 && script[0] == 0xaa && script[1] == 0x20 && script[34] == 0x87
}

pub fn is_supported_merchant_script(script: &[u8]) -> bool {
    is_p2pkh_script(script) || is_p2sh20_script(script) || is_p2sh32_script(script)
}

fn parse_token_prefix_and_script(
    field: &[u8],
) -> Result<(Option<BchToken>, Vec<u8>), TransactionError> {
    if field.first().copied() != Some(0xef) {
        return Ok((None, field.to_vec()));
    }
    if field.len() < 34 {
        return Err(TransactionError::InvalidScript);
    }
    let mut category = [0u8; 32];
    category.copy_from_slice(&field[1..33]);
    category.reverse();
    let bitfield = field[33];
    if bitfield & 0x80 != 0 {
        return Err(TransactionError::InvalidScript);
    }
    let has_amount = bitfield & 0x10 != 0;
    let has_nft = bitfield & 0x20 != 0;
    let has_commitment = bitfield & 0x40 != 0;
    let capability = match bitfield & 0x0f {
        0 => BchTokenCapability::None,
        1 => BchTokenCapability::Mutable,
        2 => BchTokenCapability::Minting,
        _ => return Err(TransactionError::InvalidScript),
    };
    if !has_nft && (has_commitment || capability != BchTokenCapability::None) {
        return Err(TransactionError::InvalidScript);
    }
    let mut offset = 34usize;
    let commitment = if has_commitment {
        let (length, next) = read_compact_uint(&field[offset..])?;
        if length == 0 || length > 40 {
            return Err(TransactionError::InvalidScript);
        }
        offset = offset
            .checked_add(next)
            .ok_or(TransactionError::InvalidScript)?;
        let end = offset
            .checked_add(length as usize)
            .ok_or(TransactionError::InvalidScript)?;
        let bytes = field.get(offset..end).ok_or(TransactionError::Truncated)?;
        offset = end;
        bytes.to_vec()
    } else {
        Vec::new()
    };
    let amount = if has_amount {
        let (amount, next) = read_compact_uint(&field[offset..])?;
        if amount == 0 || amount > i64::MAX as u64 {
            return Err(TransactionError::InvalidScript);
        }
        offset = offset
            .checked_add(next)
            .ok_or(TransactionError::InvalidScript)?;
        amount
    } else {
        0
    };
    if !has_amount && !has_nft {
        return Err(TransactionError::InvalidScript);
    }
    let token = BchToken {
        category,
        amount,
        nft: has_nft.then_some(BchNft {
            capability,
            commitment,
        }),
    };
    Ok((Some(token), field[offset..].to_vec()))
}

fn serialize_token_prefix_and_script(
    token: Option<&BchToken>,
    script_pubkey: &[u8],
) -> Result<Vec<u8>, TransactionError> {
    let mut result = serialize_token_prefix(token)?;
    result.extend_from_slice(script_pubkey);
    Ok(result)
}

fn serialize_token_prefix(token: Option<&BchToken>) -> Result<Vec<u8>, TransactionError> {
    let Some(token) = token else {
        return Ok(Vec::new());
    };
    if token.amount > i64::MAX as u64 {
        return Err(TransactionError::InvalidScript);
    }
    if token.amount == 0 && token.nft.is_none() {
        return Err(TransactionError::InvalidScript);
    }
    let mut result = vec![0xef];
    result.extend(token.category.iter().rev());
    let mut bitfield = 0u8;
    if token.amount > 0 {
        bitfield |= 0x10;
    }
    if let Some(nft) = &token.nft {
        bitfield |= match nft.capability {
            BchTokenCapability::None => 0,
            BchTokenCapability::Mutable => 1,
            BchTokenCapability::Minting => 2,
        };
        bitfield |= 0x20;
        if !nft.commitment.is_empty() {
            if nft.commitment.len() > 40 {
                return Err(TransactionError::InvalidScript);
            }
            bitfield |= 0x40;
        }
    }
    result.push(bitfield);
    if let Some(nft) = &token.nft
        && !nft.commitment.is_empty()
    {
        write_varint(nft.commitment.len() as u64, &mut result);
        result.extend_from_slice(&nft.commitment);
    }
    if token.amount > 0 {
        write_varint(token.amount, &mut result);
    }
    Ok(result)
}

fn read_compact_uint(bytes: &[u8]) -> Result<(u64, usize), TransactionError> {
    let first = *bytes.first().ok_or(TransactionError::Truncated)?;
    match first {
        0..=252 => Ok((u64::from(first), 1)),
        253 => {
            let raw = bytes.get(1..3).ok_or(TransactionError::Truncated)?;
            let value = u16::from_le_bytes([raw[0], raw[1]]) as u64;
            if value < 253 {
                return Err(TransactionError::NonCanonicalVarInt);
            }
            Ok((value, 3))
        }
        254 => {
            let raw = bytes.get(1..5).ok_or(TransactionError::Truncated)?;
            let value = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) as u64;
            if value <= u16::MAX as u64 {
                return Err(TransactionError::NonCanonicalVarInt);
            }
            Ok((value, 5))
        }
        255 => {
            let raw = bytes.get(1..9).ok_or(TransactionError::Truncated)?;
            let value = u64::from_le_bytes(raw.try_into().unwrap());
            if value <= u32::MAX as u64 {
                return Err(TransactionError::NonCanonicalVarInt);
            }
            Ok((value, 9))
        }
    }
}

pub fn make_p2pkh_script(public_key: &[u8]) -> Vec<u8> {
    p2pkh_script(&hash160(public_key))
}

pub fn push_data(data: &[u8]) -> Result<Vec<u8>, TransactionError> {
    if data.len() > 75 {
        return Err(TransactionError::InvalidScript);
    }
    let mut result = Vec::with_capacity(data.len() + 1);
    result.push(data.len() as u8);
    result.extend_from_slice(data);
    Ok(result)
}

pub fn double_sha256(data: &[u8]) -> [u8; 32] {
    let first = Sha256::digest(data);
    let second = Sha256::digest(first);
    let mut result = [0u8; 32];
    result.copy_from_slice(&second);
    result
}

fn parse_pushes(script: &[u8]) -> Result<Vec<&[u8]>, TransactionError> {
    let mut result = Vec::new();
    let mut offset = 0usize;
    while offset < script.len() {
        let opcode = script[offset];
        offset += 1;
        let length = match opcode {
            0x01..=0x4b => opcode as usize,
            0x4c => usize::from(*script.get(offset).ok_or(TransactionError::InvalidScript)?),
            0x4d => {
                let bytes = script
                    .get(offset..offset + 2)
                    .ok_or(TransactionError::InvalidScript)?;
                offset += 2;
                usize::from(u16::from_le_bytes([bytes[0], bytes[1]]))
            }
            _ => return Err(TransactionError::InvalidScript),
        };
        if opcode == 0x4c {
            offset += 1;
        }
        let end = offset
            .checked_add(length)
            .ok_or(TransactionError::InvalidScript)?;
        let value = script
            .get(offset..end)
            .ok_or(TransactionError::InvalidScript)?;
        result.push(value);
        offset = end;
    }
    Ok(result)
}

fn write_bytes(value: &[u8], output: &mut Vec<u8>) {
    write_varint(value.len() as u64, output);
    output.extend_from_slice(value);
}

fn write_varint(value: u64, output: &mut Vec<u8>) {
    if value <= 252 {
        output.push(value as u8);
    } else if value <= u16::MAX as u64 {
        output.push(253);
        output.extend_from_slice(&(value as u16).to_le_bytes());
    } else if value <= u32::MAX as u64 {
        output.push(254);
        output.extend_from_slice(&(value as u32).to_le_bytes());
    } else {
        output.push(255);
        output.extend_from_slice(&value.to_le_bytes());
    }
}

struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take(&mut self, length: usize) -> Result<&'a [u8], TransactionError> {
        let end = self
            .offset
            .checked_add(length)
            .ok_or(TransactionError::Truncated)?;
        let result = self
            .bytes
            .get(self.offset..end)
            .ok_or(TransactionError::Truncated)?;
        self.offset = end;
        Ok(result)
    }

    fn u32(&mut self) -> Result<u32, TransactionError> {
        let bytes = self.take(4)?;
        Ok(u32::from_le_bytes(
            bytes.try_into().expect("length checked"),
        ))
    }

    fn i32(&mut self) -> Result<i32, TransactionError> {
        let bytes = self.take(4)?;
        Ok(i32::from_le_bytes(
            bytes.try_into().expect("length checked"),
        ))
    }

    fn u64(&mut self) -> Result<u64, TransactionError> {
        let bytes = self.take(8)?;
        Ok(u64::from_le_bytes(
            bytes.try_into().expect("length checked"),
        ))
    }

    fn varint(&mut self) -> Result<u64, TransactionError> {
        let first = self.take(1)?[0];
        match first {
            0..=252 => Ok(u64::from(first)),
            253 => {
                let value = u16::from_le_bytes(self.take(2)?.try_into().expect("length checked"));
                if value < 253 {
                    Err(TransactionError::NonCanonicalVarInt)
                } else {
                    Ok(u64::from(value))
                }
            }
            254 => {
                let value = u32::from_le_bytes(self.take(4)?.try_into().expect("length checked"));
                if value <= u16::MAX as u32 {
                    Err(TransactionError::NonCanonicalVarInt)
                } else {
                    Ok(u64::from(value))
                }
            }
            255 => {
                let value = u64::from_le_bytes(self.take(8)?.try_into().expect("length checked"));
                if value <= u32::MAX as u64 {
                    Err(TransactionError::NonCanonicalVarInt)
                } else {
                    Ok(value)
                }
            }
        }
    }

    fn bytes(&mut self) -> Result<Vec<u8>, TransactionError> {
        let length = self.varint()? as usize;
        Ok(self.take(length)?.to_vec())
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::address::p2sh32_script;
    use x402_types::chain::ChainId;

    #[test]
    fn accepts_only_canonical_satoshi_amounts() {
        assert_eq!(parse_canonical_satoshi_amount("0").unwrap(), 0);
        assert_eq!(parse_canonical_satoshi_amount("1000").unwrap(), 1000);
        assert!(parse_canonical_satoshi_amount("01").is_err());
        assert!(parse_canonical_satoshi_amount("18446744073709551616").is_err());
    }

    #[test]
    fn serializes_and_parses_a_minimal_transaction() {
        let transaction = BchTransaction {
            version: 2,
            inputs: vec![TxInput {
                outpoint: OutPoint {
                    txid: TxId([1; 32]),
                    vout: 0,
                },
                script_sig: vec![0x01, 0x01],
                sequence: u32::MAX,
            }],
            outputs: vec![TxOutput {
                value: 1_000,
                script_pubkey: p2pkh_script(&[2; 20]),
                token: None,
            }],
            lock_time: 0,
        };
        assert_eq!(
            BchTransaction::parse(&transaction.serialize()).unwrap(),
            transaction
        );
    }

    #[test]
    fn serializes_and_parses_a_fungible_cashtoken_output() {
        let category = std::array::from_fn(|index| index as u8);
        let transaction = BchTransaction {
            version: 2,
            inputs: vec![TxInput {
                outpoint: OutPoint {
                    txid: TxId([1; 32]),
                    vout: 0,
                },
                script_sig: Vec::new(),
                sequence: u32::MAX,
            }],
            outputs: vec![TxOutput {
                value: 1_000,
                script_pubkey: p2sh32_script(&[0x22; 32]),
                token: Some(BchToken {
                    category,
                    amount: 1_000,
                    nft: None,
                }),
            }],
            lock_time: 0,
        };

        assert_eq!(
            BchTransaction::parse(&transaction.serialize()).unwrap(),
            transaction
        );
        assert_eq!(
            payment_target(
                &hex::encode(category),
                "1000",
                "cashtoken",
                Some("1000"),
                BchPolicy::default(),
            )
            .unwrap(),
            BchPaymentTarget::CashToken {
                category,
                amount: 1_000,
                merchant_value: 1_000,
                nft: None,
            }
        );
    }

    #[test]
    fn rejects_trailing_bytes() {
        let transaction = BchTransaction {
            version: 1,
            inputs: vec![TxInput {
                outpoint: OutPoint {
                    txid: TxId([0; 32]),
                    vout: 0,
                },
                script_sig: Vec::new(),
                sequence: u32::MAX,
            }],
            outputs: vec![TxOutput {
                value: 1,
                script_pubkey: p2pkh_script(&[0; 20]),
                token: None,
            }],
            lock_time: 0,
        };
        let mut raw = transaction.serialize();
        raw.push(0);
        assert!(BchTransaction::parse(&raw).is_err());
    }

    #[test]
    fn verifies_the_deterministic_interoperability_fixture() {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Fixture {
            network: String,
            amount: String,
            pay_to: String,
            source_value: String,
            #[serde(rename = "sourceScriptPubKey")]
            source_script_pubkey: String,
            raw_transaction: String,
            txid: String,
            payer: String,
            serialized_size: usize,
            fee: String,
        }

        let fixture: Fixture =
            serde_json::from_str(include_str!("../test/fixtures/bch-exact-p2pkh.json")).unwrap();
        let network =
            BchChainReference::try_from(fixture.network.parse::<ChainId>().unwrap()).unwrap();
        let transaction =
            BchTransaction::parse(&hex::decode(fixture.raw_transaction).unwrap()).unwrap();
        let pay_to = CashAddr::decode(&fixture.pay_to, network).unwrap();
        let source_output = SourceOutput {
            value: fixture.source_value.parse().unwrap(),
            script_pubkey: hex::decode(fixture.source_script_pubkey).unwrap(),
            token: None,
        };
        let target =
            payment_target("BCH", &fixture.amount, "native", None, BchPolicy::default()).unwrap();
        let verified = verify_payment(
            &transaction,
            &[source_output],
            network,
            &pay_to.locking_script(),
            &target,
            BchPolicy::default(),
        )
        .unwrap();

        assert_eq!(verified.txid.to_string(), fixture.txid);
        assert_eq!(verified.payer.to_string(), fixture.payer);
        assert_eq!(verified.fee, fixture.fee.parse::<u64>().unwrap());
        assert_eq!(transaction.serialize().len(), fixture.serialized_size);
    }

    #[test]
    fn verifies_the_shared_two_input_interoperability_fixture() {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Fixture {
            network: String,
            amount: String,
            pay_to: String,
            sources: Vec<SourceFixture>,
            raw_transaction: String,
            txid: String,
            serialized_size: usize,
            fee: String,
            payer: String,
        }

        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct SourceFixture {
            value: String,
            #[serde(rename = "scriptPubKey")]
            script_pubkey: String,
        }

        let fixture: Fixture = serde_json::from_str(include_str!(
            "../test/fixtures/bch-exact-p2pkh-two-inputs.json"
        ))
        .unwrap();
        let network =
            BchChainReference::try_from(fixture.network.parse::<ChainId>().unwrap()).unwrap();
        let transaction =
            BchTransaction::parse(&hex::decode(fixture.raw_transaction).unwrap()).unwrap();
        let pay_to = CashAddr::decode(&fixture.pay_to, network).unwrap();
        let sources = fixture
            .sources
            .iter()
            .map(|source| SourceOutput {
                value: source.value.parse().unwrap(),
                script_pubkey: hex::decode(&source.script_pubkey).unwrap(),
                token: None,
            })
            .collect::<Vec<_>>();
        let target =
            payment_target("BCH", &fixture.amount, "native", None, BchPolicy::default()).unwrap();
        let verified = verify_payment(
            &transaction,
            &sources,
            network,
            &pay_to.locking_script(),
            &target,
            BchPolicy::default(),
        )
        .unwrap();

        assert_eq!(transaction.inputs.len(), 2);
        assert_eq!(verified.txid.to_string(), fixture.txid);
        assert_eq!(verified.payer.to_string(), fixture.payer);
        assert_eq!(verified.fee, fixture.fee.parse::<u64>().unwrap());
        assert_eq!(transaction.serialize().len(), fixture.serialized_size);
    }

    #[test]
    fn verifies_the_shared_cash_token_p2sh32_fixture() {
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Fixture {
            network: String,
            asset: String,
            amount: String,
            token_output_value: String,
            pay_to: String,
            source_value: String,
            #[serde(rename = "sourceScriptPubKey")]
            source_script_pubkey: String,
            source_token: TokenFixture,
            raw_transaction: String,
            txid: String,
            serialized_size: usize,
            merchant_token_amount: String,
            change_token_amount: String,
        }

        #[derive(serde::Deserialize)]
        struct TokenFixture {
            category: String,
            amount: String,
        }

        let fixture: Fixture = serde_json::from_str(include_str!(
            "../test/fixtures/bch-exact-cashtoken-p2sh32.json"
        ))
        .unwrap();
        let network =
            BchChainReference::try_from(fixture.network.parse::<ChainId>().unwrap()).unwrap();
        let merchant = CashAddr::decode_script(&fixture.pay_to, network).unwrap();
        assert!(merchant.token_support);
        let transaction =
            BchTransaction::parse(&hex::decode(fixture.raw_transaction).unwrap()).unwrap();
        let category = parse_cash_token_category(&fixture.asset).unwrap();
        assert_eq!(fixture.source_token.category, fixture.asset);
        let target = payment_target(
            &fixture.asset,
            &fixture.amount,
            "cashtoken",
            Some(&fixture.token_output_value),
            BchPolicy::default(),
        )
        .unwrap();
        let verified = verify_payment(
            &transaction,
            &[SourceOutput {
                value: fixture.source_value.parse().unwrap(),
                script_pubkey: hex::decode(fixture.source_script_pubkey).unwrap(),
                token: Some(BchToken {
                    category,
                    amount: fixture.source_token.amount.parse().unwrap(),
                    nft: None,
                }),
            }],
            network,
            &merchant.locking_script(),
            &target,
            BchPolicy::default(),
        )
        .unwrap();

        assert_eq!(verified.txid.to_string(), fixture.txid);
        assert_eq!(transaction.serialize().len(), fixture.serialized_size);
        assert_eq!(
            transaction.outputs[0]
                .token
                .as_ref()
                .unwrap()
                .amount
                .to_string(),
            fixture.merchant_token_amount
        );
        assert_eq!(
            transaction.outputs[1]
                .token
                .as_ref()
                .unwrap()
                .amount
                .to_string(),
            fixture.change_token_amount
        );
    }
}
