//! Receiver-authorizer signature helpers.
//!
//! Mirrors `typescript/packages/mechanisms/evm/src/batch-settlement/authorizerSigner.ts`.
//!
//! A `ReceiverAuthorizerSigner` holds the EOA key designated as
//! `channelConfig.receiverAuthorizer` and produces EIP-712 signatures for:
//!
//! - `claimWithSignature(...)` — the `ClaimBatch` typed-data digest binds
//!   every voucher in the batch to the authorizer's address.
//! - `refundWithSignature(...)` — the `Refund` typed-data digest binds the
//!   channel id, refund nonce, and amount.
//!
//! The facilitator wires this in when the server delegates receiver-authorizer
//! signing to it (`channelConfig.receiverAuthorizer == authorizer.address`).
//! Servers that hold their own receiver-authorizer key supply the signatures
//! inline in the settle payload, in which case the facilitator's
//! [`ReceiverAuthorizerSigner`] is unused for that request.

use alloy_primitives::{Address, B256, Bytes, U128, U256};
use alloy_signer::SignerSync;
use alloy_signer_local::PrivateKeySigner;
use alloy_sol_types::SolStruct;

use super::abi::{
    ClaimBatch, ClaimEntry, Refund as RefundStruct, Voucher as VoucherStruct,
    VoucherClaim as VoucherClaimAbi, VoucherClaimInner as VoucherClaimInnerAbi,
};
use super::utils::{batch_settlement_domain, compute_channel_id, to_abi_channel_config};
use crate::v2_eip155_batch_settlement::types::VoucherClaim;

/// Wraps a private key dedicated to receiver-authorizer signing.
///
/// Decoupled from the chain provider's signer pool: the receiver authorizer
/// key is part of the channel identity (it shapes `channelId`), and rotating
/// it without breaking outstanding vouchers requires opening new channels.
#[derive(Debug, Clone)]
pub struct ReceiverAuthorizerSigner {
    signer: PrivateKeySigner,
}

impl ReceiverAuthorizerSigner {
    /// Constructs a new signer from a raw secp256k1 private key.
    pub fn new(signer: PrivateKeySigner) -> Self {
        Self { signer }
    }

    /// The receiver-authorizer EOA address — published in
    /// `SupportedResponse.kinds[].extra.receiverAuthorizer`.
    pub fn address(&self) -> Address {
        self.signer.address()
    }

    /// Produces an EIP-712 signature over the `ClaimBatch` digest for the
    /// given claim rows.
    pub fn sign_claim_batch(
        &self,
        claims: &[VoucherClaim],
        chain_id: u64,
    ) -> Result<Bytes, alloy_signer::Error> {
        let entries: Vec<ClaimEntry> = claims
            .iter()
            .map(|c| ClaimEntry {
                channelId: compute_channel_id(&c.voucher.channel, chain_id),
                maxClaimableAmount: c.voucher.max_claimable_amount.0.to::<u128>(),
                totalClaimed: c.total_claimed.0.to::<u128>(),
            })
            .collect();
        let batch = ClaimBatch { claims: entries };
        let digest = batch.eip712_signing_hash(&batch_settlement_domain(chain_id));
        let signature = self.signer.sign_hash_sync(&digest)?;
        Ok(signature.as_bytes().to_vec().into())
    }

    /// Produces an EIP-712 signature over the `Refund` digest.
    pub fn sign_refund(
        &self,
        channel_id: B256,
        amount: U128,
        nonce: U256,
        chain_id: u64,
    ) -> Result<Bytes, alloy_signer::Error> {
        let refund = RefundStruct {
            channelId: channel_id,
            nonce,
            amount: amount.to::<u128>(),
        };
        let digest = refund.eip712_signing_hash(&batch_settlement_domain(chain_id));
        let signature = self.signer.sign_hash_sync(&digest)?;
        Ok(signature.as_bytes().to_vec().into())
    }
}

/// Builds the `VoucherClaim` ABI tuple expected by `claim` / `claimWithSignature`.
pub fn to_abi_voucher_claims(claims: &[VoucherClaim]) -> Vec<VoucherClaimAbi> {
    claims
        .iter()
        .map(|c| VoucherClaimAbi {
            voucher: VoucherClaimInnerAbi {
                channel: to_abi_channel_config(&c.voucher.channel),
                maxClaimableAmount: c.voucher.max_claimable_amount.0.to::<u128>(),
            },
            signature: c.signature.clone(),
            totalClaimed: c.total_claimed.0.to::<u128>(),
        })
        .collect()
}

/// EIP-712 digest helper used to cross-check that a server-supplied claim batch
/// signature came from the expected receiver authorizer.
pub fn compute_claim_batch_digest(claims: &[VoucherClaim], chain_id: u64) -> B256 {
    let entries: Vec<ClaimEntry> = claims
        .iter()
        .map(|c| ClaimEntry {
            channelId: compute_channel_id(&c.voucher.channel, chain_id),
            maxClaimableAmount: c.voucher.max_claimable_amount.0.to::<u128>(),
            totalClaimed: c.total_claimed.0.to::<u128>(),
        })
        .collect();
    ClaimBatch { claims: entries }.eip712_signing_hash(&batch_settlement_domain(chain_id))
}

/// EIP-712 digest helper for cross-checking a refund authorizer signature.
pub fn compute_refund_digest(channel_id: B256, amount: U128, nonce: U256, chain_id: u64) -> B256 {
    RefundStruct {
        channelId: channel_id,
        nonce,
        amount: amount.to::<u128>(),
    }
    .eip712_signing_hash(&batch_settlement_domain(chain_id))
}

/// EIP-712 digest helper for cross-checking a voucher signature.
pub fn compute_voucher_struct_digest(
    channel_id: B256,
    max_claimable_amount: U128,
    chain_id: u64,
) -> B256 {
    VoucherStruct {
        channelId: channel_id,
        maxClaimableAmount: max_claimable_amount.to::<u128>(),
    }
    .eip712_signing_hash(&batch_settlement_domain(chain_id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::v2_eip155_batch_settlement::types::{
        ChannelConfig as WireChannelConfig, VoucherClaim, VoucherClaimVoucher,
    };
    use alloy_primitives::B256;

    fn sample_config() -> WireChannelConfig {
        WireChannelConfig {
            payer: "0x0000000000000000000000000000000000000001"
                .parse()
                .unwrap(),
            payer_authorizer: "0x0000000000000000000000000000000000000002"
                .parse()
                .unwrap(),
            receiver: "0x0000000000000000000000000000000000000003"
                .parse()
                .unwrap(),
            receiver_authorizer: "0x0000000000000000000000000000000000000004"
                .parse()
                .unwrap(),
            token: "0x0000000000000000000000000000000000000005"
                .parse()
                .unwrap(),
            withdraw_delay: 900,
            salt: B256::ZERO,
        }
    }

    #[test]
    fn sign_refund_signature_verifies_against_authorizer_address() {
        let signer = PrivateKeySigner::random();
        let authorizer_addr = signer.address();
        let authorizer = ReceiverAuthorizerSigner::new(signer);

        let channel_id = B256::repeat_byte(0xaa);
        let amount = U128::from(500u128);
        let nonce = U256::from(1u64);
        let chain_id = 84532u64;

        let sig_bytes = authorizer
            .sign_refund(channel_id, amount, nonce, chain_id)
            .unwrap();
        let digest = compute_refund_digest(channel_id, amount, nonce, chain_id);

        let sig = alloy_primitives::Signature::from_raw(&sig_bytes)
            .unwrap()
            .normalized_s();
        let recovered = sig.recover_address_from_prehash(&digest).unwrap();
        assert_eq!(recovered, authorizer_addr);
    }

    #[test]
    fn sign_claim_batch_signature_verifies_against_authorizer_address() {
        let signer = PrivateKeySigner::random();
        let authorizer_addr = signer.address();
        let authorizer = ReceiverAuthorizerSigner::new(signer);
        let chain_id = 84532u64;

        let claims = vec![VoucherClaim {
            voucher: VoucherClaimVoucher {
                channel: sample_config(),
                max_claimable_amount: U128::from(5_000u128).into(),
            },
            // The voucher signature itself doesn't matter for the claim-batch
            // authorizer digest — only the entries do.
            signature: alloy_primitives::Bytes::from_static(&[]),
            total_claimed: U128::from(5_000u128).into(),
        }];

        let sig_bytes = authorizer.sign_claim_batch(&claims, chain_id).unwrap();
        let digest = compute_claim_batch_digest(&claims, chain_id);

        let sig = alloy_primitives::Signature::from_raw(&sig_bytes)
            .unwrap()
            .normalized_s();
        let recovered = sig.recover_address_from_prehash(&digest).unwrap();
        assert_eq!(recovered, authorizer_addr);
    }

    /// Two different chains must yield different `Refund` digests so a refund
    /// signed for chain A cannot be replayed against chain B.
    #[test]
    fn refund_digest_is_chain_bound() {
        let channel_id = B256::repeat_byte(0xbb);
        let amount = U128::from(500u128);
        let nonce = U256::from(2u64);
        let base = compute_refund_digest(channel_id, amount, nonce, 8453);
        let optimism = compute_refund_digest(channel_id, amount, nonce, 10);
        assert_ne!(base, optimism);
    }

    #[test]
    fn voucher_struct_digest_is_chain_bound() {
        let channel_id = B256::repeat_byte(0xcc);
        let amount = U128::from(1_000u128);
        let base = compute_voucher_struct_digest(channel_id, amount, 8453);
        let optimism = compute_voucher_struct_digest(channel_id, amount, 10);
        assert_ne!(base, optimism);
    }

    #[test]
    fn to_abi_voucher_claims_preserves_amounts_and_signatures() {
        let claims = vec![VoucherClaim {
            voucher: VoucherClaimVoucher {
                channel: sample_config(),
                max_claimable_amount: U128::from(7_000u128).into(),
            },
            signature: alloy_primitives::Bytes::from_static(&[0xab, 0xcd]),
            total_claimed: U128::from(7_000u128).into(),
        }];
        let abi = to_abi_voucher_claims(&claims);
        assert_eq!(abi.len(), 1);
        assert_eq!(abi[0].voucher.maxClaimableAmount, 7_000u128);
        assert_eq!(abi[0].totalClaimed, 7_000u128);
        assert_eq!(
            abi[0].signature,
            alloy_primitives::Bytes::from_static(&[0xab, 0xcd])
        );
    }
}
