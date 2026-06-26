//! Calldata-encoding helpers for the batch-settlement deposit collectors.
//!
//! Mirrors `typescript/packages/mechanisms/evm/src/batch-settlement/encoding.ts`
//! exactly. The `x402BatchSettlement.deposit(...)` entry point takes an
//! opaque `bytes collectorData`; the shape of those bytes depends on which
//! deposit collector contract is being targeted.

use alloy_primitives::{B256, Bytes, U256, keccak256};
use alloy_sol_types::{SolValue, sol};

sol! {
    /// `keccak256(abi.encode(channelId, salt))` — the ERC-3009 nonce that binds
    /// a deposit authorization to a specific channel + per-deposit salt.
    struct Erc3009DepositNonceInput {
        bytes32 channelId;
        uint256 salt;
    }

    /// `abi.encode(validAfter, validBefore, salt, signature)` — the
    /// `collectorData` payload for `ERC3009DepositCollector.collect(...)`.
    struct Erc3009CollectorData {
        uint256 validAfter;
        uint256 validBefore;
        uint256 salt;
        bytes signature;
    }

    /// `abi.encode(value, deadline, v, r, s)` — the optional EIP-2612 permit
    /// segment consumed by `Permit2DepositCollector` for atomic approvals.
    struct Eip2612PermitData {
        uint256 value;
        uint256 deadline;
        uint8 v;
        bytes32 r;
        bytes32 s;
    }

    /// `abi.encode(nonce, deadline, permit2Signature, eip2612PermitData)` — the
    /// `collectorData` payload for `Permit2DepositCollector.collect(...)`.
    /// `eip2612PermitData` is `0x` when no EIP-2612 permit is bundled.
    struct Permit2CollectorData {
        uint256 nonce;
        uint256 deadline;
        bytes permit2Signature;
        bytes eip2612PermitData;
    }
}

/// Computes the ERC-3009 nonce used by the deposit collector:
/// `keccak256(abi.encode(channelId, salt))`.
pub fn build_erc3009_deposit_nonce(channel_id: B256, salt: B256) -> B256 {
    let input = Erc3009DepositNonceInput {
        channelId: channel_id,
        salt: U256::from_be_bytes(salt.0),
    };
    keccak256(input.abi_encode())
}

/// Encodes the `collectorData` payload for `ERC3009DepositCollector.collect()`:
/// `abi.encode(validAfter, validBefore, salt, signature)`.
pub fn build_erc3009_collector_data(
    valid_after: U256,
    valid_before: U256,
    salt: B256,
    signature: &Bytes,
) -> Bytes {
    // The x402 collector contract decodes this as
    // `(uint256,uint256,uint256,bytes)`, so encode a parameter sequence rather
    // than a single wrapped struct value.
    Bytes::from(
        (
            valid_after,
            valid_before,
            U256::from_be_bytes(salt.0),
            signature.clone(),
        )
            .abi_encode_sequence(),
    )
}

/// Encodes optional EIP-2612 permit data consumed by `Permit2DepositCollector`.
pub fn build_eip2612_permit_data(value: U256, deadline: U256, v: u8, r: B256, s: B256) -> Bytes {
    let data = Eip2612PermitData {
        value,
        deadline,
        v,
        r,
        s,
    };
    Bytes::from(data.abi_encode())
}

/// Encodes the `collectorData` payload for `Permit2DepositCollector.collect()`.
///
/// Pass an empty `eip2612_permit_data` (`Bytes::new()`) when no EIP-2612
/// permit is bundled — the contract treats `0x` as "no permit".
pub fn build_permit2_collector_data(
    nonce: U256,
    deadline: U256,
    permit2_signature: &Bytes,
    eip2612_permit_data: &Bytes,
) -> Bytes {
    // The x402 collector contract decodes this as
    // `(uint256,uint256,bytes,bytes)`, so encode a parameter sequence rather
    // than a single wrapped struct value.
    Bytes::from(
        (
            nonce,
            deadline,
            permit2_signature.clone(),
            eip2612_permit_data.clone(),
        )
            .abi_encode_sequence(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{b256, hex};

    #[test]
    fn erc3009_deposit_nonce_matches_keccak_of_abi_encoded_pair() {
        // Independently compute the expected nonce by hashing the manually
        // constructed `abi.encode(bytes32, uint256)` payload: two 32-byte
        // big-endian words.
        let channel_id =
            b256!("0x1111111111111111111111111111111111111111111111111111111111111111");
        let salt = b256!("0x2222222222222222222222222222222222222222222222222222222222222222");
        let mut packed = [0u8; 64];
        packed[..32].copy_from_slice(channel_id.as_slice());
        packed[32..].copy_from_slice(salt.as_slice());
        let expected = keccak256(packed);

        assert_eq!(build_erc3009_deposit_nonce(channel_id, salt), expected);
    }

    #[test]
    fn erc3009_collector_data_round_trips_via_abi() {
        let bytes = build_erc3009_collector_data(
            U256::from(0u64),
            U256::from(1_770_000_000u64),
            b256!("0x0000000000000000000000000000000000000000000000000000000000000077"),
            &Bytes::from_static(&hex!("deadbeef")),
        );
        assert_eq!(bytes.len(), 192);
        let decoded = Erc3009CollectorData::abi_decode_sequence(&bytes).unwrap();
        assert_eq!(decoded.validAfter, U256::ZERO);
        assert_eq!(decoded.validBefore, U256::from(1_770_000_000u64));
        assert_eq!(decoded.salt, U256::from(0x77u64));
        assert_eq!(decoded.signature, Bytes::from_static(&hex!("deadbeef")));
    }

    #[test]
    fn permit2_collector_data_round_trips_via_abi() {
        let bytes = build_permit2_collector_data(
            U256::from(42u64),
            U256::from(1_770_000_000u64),
            &Bytes::from_static(&hex!("abcd")),
            &Bytes::new(),
        );
        assert_eq!(bytes.len(), 224);
        let decoded = Permit2CollectorData::abi_decode_sequence(&bytes).unwrap();
        assert_eq!(decoded.nonce, U256::from(42u64));
        assert_eq!(decoded.deadline, U256::from(1_770_000_000u64));
        assert_eq!(decoded.permit2Signature, Bytes::from_static(&hex!("abcd")));
        assert_eq!(decoded.eip2612PermitData, Bytes::new());
    }

    #[test]
    fn eip2612_permit_data_round_trips_via_abi() {
        let bytes = build_eip2612_permit_data(
            U256::from(1_000u64),
            U256::from(1_770_000_000u64),
            27,
            b256!("0x1111111111111111111111111111111111111111111111111111111111111111"),
            b256!("0x2222222222222222222222222222222222222222222222222222222222222222"),
        );
        let decoded = Eip2612PermitData::abi_decode(&bytes).unwrap();
        assert_eq!(decoded.value, U256::from(1_000u64));
        assert_eq!(decoded.deadline, U256::from(1_770_000_000u64));
        assert_eq!(decoded.v, 27);
    }
}
