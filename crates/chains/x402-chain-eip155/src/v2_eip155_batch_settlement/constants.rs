//! Canonical addresses, EIP-712 type hashes, and protocol bounds for the
//! `batch-settlement` EVM scheme.
//!
//! These mirror `typescript/packages/mechanisms/evm/src/batch-settlement/constants.ts`
//! exactly. The `x402BatchSettlement`, `ERC3009DepositCollector`, and
//! `Permit2DepositCollector` contracts are deployed via CREATE2 at the same
//! addresses across every supported EVM chain.

use alloy_primitives::{Address, address};

/// Scheme identifier for the batch-settlement payment scheme.
pub const BATCH_SETTLEMENT_SCHEME: &str = "batch-settlement";

/// Deployed address of the `x402BatchSettlement` contract.
pub const BATCH_SETTLEMENT_ADDRESS: Address =
    address!("0x4020074e9dF2ce1deE5A9C1b5c3f541D02a10003");

/// Deployed address of the `ERC3009DepositCollector` contract.
pub const ERC3009_DEPOSIT_COLLECTOR_ADDRESS: Address =
    address!("0x4020806089470a89826cB9fB1f4059150b550004");

/// Deployed address of the `Permit2DepositCollector` contract.
pub const PERMIT2_DEPOSIT_COLLECTOR_ADDRESS: Address =
    address!("0x4020425FAf3B746C082C2f942b4E5159887B0005");

/// Minimum withdraw delay in seconds (15 minutes), matching the onchain constant.
pub const MIN_WITHDRAW_DELAY: u64 = 900;

/// Maximum withdraw delay in seconds (30 days), matching the onchain constant.
pub const MAX_WITHDRAW_DELAY: u64 = 2_592_000;

/// EIP-712 domain name shared across all batch-settlement typed-data signatures.
pub const BATCH_SETTLEMENT_DOMAIN_NAME: &str = "x402 Batch Settlement";

/// EIP-712 domain version shared across all batch-settlement typed-data signatures.
pub const BATCH_SETTLEMENT_DOMAIN_VERSION: &str = "1";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_addresses_match_spec() {
        // Sanity checks that the canonical addresses literally match the spec.
        // These addresses are referenced by the deployed contracts and must be
        // identical across every EVM chain — drift would break interop with
        // every other batch-settlement implementation.
        assert_eq!(
            format!("{:#x}", BATCH_SETTLEMENT_ADDRESS),
            "0x4020074e9df2ce1dee5a9c1b5c3f541d02a10003"
        );
        assert_eq!(
            format!("{:#x}", ERC3009_DEPOSIT_COLLECTOR_ADDRESS),
            "0x4020806089470a89826cb9fb1f4059150b550004"
        );
        assert_eq!(
            format!("{:#x}", PERMIT2_DEPOSIT_COLLECTOR_ADDRESS),
            "0x4020425faf3b746c082c2f942b4e5159887b0005"
        );
    }

    #[test]
    fn withdraw_delay_bounds_match_spec() {
        assert_eq!(MIN_WITHDRAW_DELAY, 15 * 60);
        assert_eq!(MAX_WITHDRAW_DELAY, 30 * 24 * 60 * 60);
    }
}
