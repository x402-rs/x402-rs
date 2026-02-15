//! Permit2 type definitions and contract bindings for V2 EIP-155 exact scheme.
//!
//! This module provides:
//! - Solidity struct definitions for EIP-712 signing
//! - Contract addresses for Permit2 and x402Permit2Proxy
//!
//! # EIP-712 Typed Data
//!
//! The Permit2 signature follows the EIP-712 standard with the following domain separator:
//!
//! ```text
//! DOMAIN_SEPARATOR = hashStruct(
//!     EIP712Domain({
//!         name: "Permit2",
//!         version: "0",
//!         chainId: CHAIN_ID,
//!         verifyingContract: PERMIT2_ADDRESS
//!     })
//! )
//! ```
//!
//! The PermitTransferFrom struct is hashed according to:
//!
//! ```text
//! PERMIT_TRANSFER_FROM_TYPEHASH = keccak256(
//!     "PermitTransferFrom(TokenPermissions permitted,address spender,uint256 nonce,uint256 deadline)TokenPermissions(address token,uint256 amount)"
//! )
//! ```

use alloy_primitives::{Address, B256, U256, address};
use serde::{Deserialize, Serialize};

// TODO IS this all needed??

#[cfg(any(feature = "facilitator", feature = "client"))]
use alloy_sol_types::sol;

/// Canonical Permit2 contract address (Uniswap Permit2).
/// This is the same across all EVM chains.
pub const CANONICAL_PERMIT2_ADDRESS: Address =
    address!("0x000000000022D473030F116dDEE9F6B43aC78BA3");

/// x402Permit2Proxy contract address for V2 exact scheme.
/// Deployed using CREATE2 for deterministic addresses.
pub const EXACT_PERMIT2_PROXY_ADDRESS: Address =
    address!("0x4020615294c913F045dc10f0a5cdEbd86c280001");

#[cfg(any(feature = "facilitator", feature = "client"))]
sol! {
    /// Permit2 PermitTransferFrom structure for EIP-712 signing.
    ///
    /// This struct represents the authorization to transfer tokens via Permit2.
    /// The user signs this struct with their private key to authorize the transfer.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let permit = PermitTransferFrom {
    ///     permitted: TokenPermissions {
    ///         token: usdc_address,
    ///         amount: U256::from(1_000_000),
    ///     },
    ///     spender: proxy_address,
    ///     nonce: U256::from(1),
    ///     deadline: U256::from(u64::MAX),
    /// };
    /// ```
    #[derive(Serialize, Deserialize)]
    struct PermitTransferFrom {
        TokenPermissions permitted;
        address spender;
        uint256 nonce;
        uint256 deadline;
    }

    /// Token permissions for Permit2 transfers.
    #[derive(Serialize, Deserialize)]
    struct TokenPermissions {
        address token;
        uint256 amount;
    }

    /// x402 Witness structure for Permit2 transfers.
    ///
    /// This witness data is specific to the x402 protocol and is included
    /// in the Permit2 authorization to ensure the transfer is for a valid
    /// x402 payment.
    #[derive(Serialize, Deserialize)]
    struct Witness {
        address to;
        uint256 validAfter;
        bytes extra;
    }
}

// /// Permitted struct for allowance transfer (IAllowanceTransfer).
// #[derive(Debug, Clone, Serialize, Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct PermitDetails {
//     pub token: Address,
//     pub amount: U256,
//     pub expiration: u64,
//     pub nonce: u64,
// }
//
// /// Allowance transfer parameters (IAllowanceTransfer.PermitSingle).
// #[derive(Debug, Clone, Serialize, Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct PermitSingle {
//     pub details: PermitDetails,
//     pub spender: Address,
//     pub sig_deadline: U256,
// }
//
// /// Transfer details for permit transfer (ISignatureTransfer.SignatureTransferDetails).
// #[derive(Debug, Clone, Serialize, Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct SignatureTransferDetails {
//     pub to: Address,
//     pub requested_amount: U256,
// }
//
// /// Permit transfer parameters (ISignatureTransfer.PermitTransferFrom).
// #[derive(Debug, Clone, Serialize, Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct SignaturePermitTransferFrom {
//     pub permitted: SignatureTokenPermissions,
//     pub nonce: U256,
//     pub deadline: U256,
// }
//
// /// Token permissions for signature transfer (ISignatureTransfer.TokenPermissions).
// #[derive(Debug, Clone, Serialize, Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct SignatureTokenPermissions {
//     pub token: Address,
//     pub amount: U256,
// }
//
// #[cfg(test)]
// mod tests {
//     use super::*;
//
//     #[test]
//     fn test_permit2_addresses() {
//         // Verify canonical Permit2 address length (20 bytes)
//         assert_eq!(CANONICAL_PERMIT2_ADDRESS.as_slice().len(), 20);
//
//         // Verify x402 proxy address length (20 bytes)
//         assert_eq!(EXACT_PERMIT2_PROXY_ADDRESS.as_slice().len(), 20);
//     }
//
//     #[test]
//     fn test_permit_details_serde() {
//         let details = PermitDetails {
//             token: Address::ZERO,
//             amount: U256::from(1000),
//             expiration: 0,
//             nonce: 0,
//         };
//
//         let json = serde_json::to_string(&details).unwrap();
//         let parsed: PermitDetails = serde_json::from_str(&json).unwrap();
//
//         assert_eq!(details.token, parsed.token);
//         assert_eq!(details.amount, parsed.amount);
//     }
//
//     #[test]
//     fn test_signature_transfer_details_serde() {
//         let details = SignatureTransferDetails {
//             to: Address::ZERO,
//             requested_amount: U256::from(500),
//         };
//
//         let json = serde_json::to_string(&details).unwrap();
//         let parsed: SignatureTransferDetails = serde_json::from_str(&json).unwrap();
//
//         assert_eq!(details.to, parsed.to);
//         assert_eq!(details.requested_amount, parsed.requested_amount);
//     }
//
//     #[test]
//     fn test_signature_permit_transfer_from_serde() {
//         let permit = SignaturePermitTransferFrom {
//             permitted: SignatureTokenPermissions {
//                 token: Address::ZERO,
//                 amount: U256::from(1000),
//             },
//             nonce: U256::from(5),
//             deadline: U256::from(u64::MAX),
//         };
//
//         let json = serde_json::to_string(&permit).unwrap();
//         let parsed: SignaturePermitTransferFrom = serde_json::from_str(&json).unwrap();
//
//         assert_eq!(permit.permitted.token, parsed.permitted.token);
//         assert_eq!(permit.permitted.amount, parsed.permitted.amount);
//         assert_eq!(permit.nonce, parsed.nonce);
//     }
// }
