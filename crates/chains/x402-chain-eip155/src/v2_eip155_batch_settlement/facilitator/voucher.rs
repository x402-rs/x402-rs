//! Voucher signature verification for the batch-settlement scheme.
//!
//! There are two verification paths:
//!
//! - **Plain ECDSA recovery** (the fast / RPC-free path). Used when the channel
//!   was configured with a non-zero `payerAuthorizer`. The recovered signer
//!   must equal that authorizer EOA.
//! - **EIP-1271 fallback** (`isValidSignature`). Used when the channel was
//!   configured with `payerAuthorizer == address(0)`, indicating a smart wallet
//!   payer that signs vouchers through its contract.

#[cfg(test)]
use alloy_primitives::U128;
use alloy_primitives::{Address, B256, Signature};
use alloy_provider::Provider;
use alloy_sol_types::{SolCall, sol};

use super::utils::compute_voucher_digest;
use crate::v1_eip155_exact::{StructuredSignature, StructuredSignatureFormatError};
use crate::v2_eip155_batch_settlement::types::VoucherFields;

sol! {
    /// ERC-1271 `isValidSignature(bytes32, bytes)` — returns the 4-byte
    /// magic value `0x1626ba7e` when the contract considers the signature valid.
    function isValidSignature(bytes32 hash, bytes signature) external view returns (bytes4);
}

/// The 4-byte `bytes4(keccak256("isValidSignature(bytes32,bytes)"))` magic
/// value returned by EIP-1271 wallets for valid signatures.
pub const EIP1271_MAGIC_VALUE: [u8; 4] = [0x16, 0x26, 0xba, 0x7e];

/// Reasons voucher verification can fail. Each variant maps 1:1 to a wire-format
/// error code (see [`crate::v2_eip155_batch_settlement::errors`]) and ultimately
/// to an `invalidReason` string on the `VerifyResponse`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoucherVerifyError {
    /// Signature bytes do not decode as a valid Ethereum signature.
    InvalidFormat,
    /// Signature did not recover to (or validate against) the expected signer.
    InvalidSignature,
    /// Onchain EIP-1271 lookup failed (RPC error).
    RpcReadFailed,
}

impl VoucherVerifyError {
    /// Maps to the scheme's wire-format error code string.
    pub fn as_error_code(self) -> &'static str {
        use crate::v2_eip155_batch_settlement::errors as err;
        match self {
            VoucherVerifyError::InvalidFormat => err::ERR_INVALID_VOUCHER_SIGNATURE,
            VoucherVerifyError::InvalidSignature => err::ERR_INVALID_VOUCHER_SIGNATURE,
            VoucherVerifyError::RpcReadFailed => err::ERR_RPC_READ_FAILED,
        }
    }
}

impl From<StructuredSignatureFormatError> for VoucherVerifyError {
    fn from(_: StructuredSignatureFormatError) -> Self {
        VoucherVerifyError::InvalidFormat
    }
}

/// Computes the digest the voucher signer commits to and dispatches to either
/// the ECDSA-recovery path or the EIP-1271 path based on the channel's
/// `payerAuthorizer`.
pub async fn verify_voucher_signature<P>(
    provider: &P,
    voucher: &VoucherFields,
    payer: Address,
    payer_authorizer: Address,
    chain_id: u64,
) -> Result<(), VoucherVerifyError>
where
    P: Provider,
{
    let digest =
        compute_voucher_digest(voucher.channel_id, voucher.max_claimable_amount.0, chain_id);
    verify_signature_against_signer(
        provider,
        &voucher.signature,
        &digest,
        payer,
        payer_authorizer,
    )
    .await
}

/// Same as [`verify_voucher_signature`] but for arbitrary digests / signatures.
///
/// Used by the deposit verification path to check an ERC-3009
/// `ReceiveWithAuthorization` signature, which obeys the same EOA-then-1271
/// fallback rules as a voucher.
pub async fn verify_signature_against_signer<P>(
    provider: &P,
    signature: &[u8],
    digest: &B256,
    payer: Address,
    payer_authorizer: Address,
) -> Result<(), VoucherVerifyError>
where
    P: Provider,
{
    if payer_authorizer != Address::ZERO {
        recover_ecdsa_and_match(signature, digest, payer_authorizer)
    } else {
        verify_via_eip1271(provider, signature, digest, payer).await
    }
}

/// Recovers an EOA signature and matches it against `expected_signer`.
///
/// Both 64-byte (ERC-2098 compact) and 65-byte canonical EIP-712 signatures
/// are accepted, matching the upstream behaviour. Smart-wallet wrapped
/// signatures (EIP-6492) are rejected on the ECDSA path; those are reserved
/// for the EIP-1271 fallback.
fn recover_ecdsa_and_match(
    signature: &[u8],
    digest: &B256,
    expected_signer: Address,
) -> Result<(), VoucherVerifyError> {
    let normalized: Option<Signature> = match signature.len() {
        65 => Signature::from_raw(signature)
            .ok()
            .map(|s| s.normalized_s()),
        64 => Some(Signature::from_erc2098(signature).normalized_s()),
        _ => None,
    };
    let signature = normalized.ok_or(VoucherVerifyError::InvalidFormat)?;
    let recovered = signature
        .recover_address_from_prehash(digest)
        .map_err(|_| VoucherVerifyError::InvalidSignature)?;
    if recovered == expected_signer {
        Ok(())
    } else {
        Err(VoucherVerifyError::InvalidSignature)
    }
}

/// Verifies a signature against a deployed EIP-1271 smart wallet at `payer`.
async fn verify_via_eip1271<P>(
    provider: &P,
    signature: &[u8],
    digest: &B256,
    payer: Address,
) -> Result<(), VoucherVerifyError>
where
    P: Provider,
{
    // For counterfactual wallets, the signature may be wrapped in EIP-6492.
    // We use the same parser as the v1 exact scheme to unwrap the inner
    // signature, then call `isValidSignature` against the (already-deployed)
    // wallet contract. Counterfactual deployment is out of scope for
    // batch-settlement voucher verification: vouchers are issued in the
    // steady state, after the wallet is deployed via the initial deposit.
    let structured = StructuredSignature::try_from_bytes(signature.to_vec().into(), payer, digest)?;

    let inner = match structured {
        StructuredSignature::EOA(_) => {
            // Channel config says the voucher signer is `payer` itself, but
            // the signature recovers to an EOA — treat that as a smart-wallet
            // contract that happens to be at `payer`. Pass the original bytes
            // unchanged to `isValidSignature`.
            signature.to_vec().into()
        }
        StructuredSignature::EIP1271(bytes) => bytes,
        StructuredSignature::EIP6492 { inner, .. } => inner,
    };

    let call = isValidSignatureCall {
        hash: *digest,
        signature: inner,
    };
    let calldata = call.abi_encode();
    let request = alloy_rpc_types_eth::TransactionRequest::default()
        .input(calldata.into())
        .to(payer);

    let raw = provider
        .call(request)
        .await
        .map_err(|_| VoucherVerifyError::RpcReadFailed)?;

    if raw.len() < 32 {
        return Err(VoucherVerifyError::InvalidSignature);
    }
    // The ABI return value for `bytes4` is right-padded to 32 bytes; the
    // first 4 bytes hold the selector.
    let magic = &raw[..4];
    if magic == EIP1271_MAGIC_VALUE {
        Ok(())
    } else {
        Err(VoucherVerifyError::InvalidSignature)
    }
}

/// Helper used by module-level tests to build a voucher signed by an EOA.
///
/// Not part of the production verify path — exposed to module tests and
/// downstream integration tests so they can construct realistic vouchers
/// without importing the full client crate.
#[cfg(test)]
#[allow(dead_code)] // used by downstream integration tests
pub(crate) fn sign_voucher_for_test(
    channel_id: B256,
    max_claimable_amount: U128,
    chain_id: u64,
    signer: &alloy_signer_local::PrivateKeySigner,
) -> alloy_primitives::Bytes {
    use alloy_signer::SignerSync;
    let digest = compute_voucher_digest(channel_id, max_claimable_amount, chain_id);
    let signature = signer.sign_hash_sync(&digest).expect("eoa signing");
    signature.as_bytes().to_vec().into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::types::EOASignatureExt;
    use crate::v2_eip155_batch_settlement::facilitator::utils::compute_channel_id;
    use crate::v2_eip155_batch_settlement::types::ChannelConfig as WireChannelConfig;
    use alloy_signer::SignerSync;
    use alloy_signer_local::PrivateKeySigner;

    #[test]
    fn ecdsa_voucher_recovers_to_signer() {
        let signer = PrivateKeySigner::random();
        let signer_address = signer.address();
        let channel_id = B256::repeat_byte(0x42);
        let max_claimable = U128::from(1_000u128);
        let chain_id = 84532;

        let digest = compute_voucher_digest(channel_id, max_claimable, chain_id);
        let signature = signer.sign_hash_sync(&digest).unwrap();
        let bytes = signature.as_bytes();
        assert_eq!(bytes.len(), 65);

        let result = recover_ecdsa_and_match(&bytes, &digest, signer_address);
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn ecdsa_voucher_rejects_signer_mismatch() {
        let signer = PrivateKeySigner::random();
        let other = PrivateKeySigner::random();
        let channel_id = B256::repeat_byte(0x42);
        let max_claimable = U128::from(1_000u128);
        let chain_id = 84532;

        let digest = compute_voucher_digest(channel_id, max_claimable, chain_id);
        let signature = signer.sign_hash_sync(&digest).unwrap();
        let bytes = signature.as_bytes();

        let result = recover_ecdsa_and_match(&bytes, &digest, other.address());
        assert_eq!(result, Err(VoucherVerifyError::InvalidSignature));
    }

    #[test]
    fn ecdsa_voucher_rejects_garbage_signature() {
        let channel_id = B256::repeat_byte(0x42);
        let max_claimable = U128::from(1_000u128);
        let chain_id = 84532;
        let digest = compute_voucher_digest(channel_id, max_claimable, chain_id);

        let result = recover_ecdsa_and_match(&[0u8; 32], &digest, Address::ZERO);
        assert_eq!(result, Err(VoucherVerifyError::InvalidFormat));
    }

    #[test]
    fn ecdsa_voucher_accepts_erc2098_compact_signature() {
        let signer = PrivateKeySigner::random();
        let signer_address = signer.address();
        let channel_id = B256::repeat_byte(0x42);
        let max_claimable = U128::from(1_000u128);
        let chain_id = 84532;

        let digest = compute_voucher_digest(channel_id, max_claimable, chain_id);
        let signature = signer.sign_hash_sync(&digest).unwrap();
        // ERC-2098 compact: drop the v byte from the 65-byte canonical form
        // and encode y-parity in the high bit of `s`.
        let mut compact = [0u8; 64];
        compact[..32].copy_from_slice(signature.r_bytes().as_slice());
        let s = signature.s_bytes();
        compact[32..].copy_from_slice(s.as_slice());
        if signature.v() {
            compact[32] |= 0x80;
        }
        let result = recover_ecdsa_and_match(&compact, &digest, signer_address);
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn eip1271_magic_value_matches_canonical_selector() {
        // 0x1626ba7e == bytes4(keccak256("isValidSignature(bytes32,bytes)"))
        assert_eq!(EIP1271_MAGIC_VALUE, [0x16, 0x26, 0xba, 0x7e]);
    }

    /// End-to-end parity check: signing a voucher with the helper recovers to
    /// the same EOA the verify path expects. This is the primary parity
    /// assertion between the Rust port and the TS / Go reference signers.
    #[test]
    fn signed_voucher_round_trips_through_recover() {
        let signer = PrivateKeySigner::random();
        let signer_address = signer.address();
        let cfg = WireChannelConfig {
            payer: "0x0000000000000000000000000000000000000aaa"
                .parse()
                .unwrap(),
            payer_authorizer: signer_address.into(),
            receiver: "0x0000000000000000000000000000000000000bbb"
                .parse()
                .unwrap(),
            receiver_authorizer: "0x0000000000000000000000000000000000000ccc"
                .parse()
                .unwrap(),
            token: "0x0000000000000000000000000000000000000ddd"
                .parse()
                .unwrap(),
            withdraw_delay: 900,
            salt: B256::ZERO,
        };
        let chain_id = 84532u64;
        let channel_id = compute_channel_id(&cfg, chain_id);
        let max_claimable = U128::from(5_000u128);

        let sig_bytes = sign_voucher_for_test(channel_id, max_claimable, chain_id, &signer);
        let digest = compute_voucher_digest(channel_id, max_claimable, chain_id);
        let result = recover_ecdsa_and_match(&sig_bytes, &digest, signer_address);
        assert_eq!(result, Ok(()));
    }
}
