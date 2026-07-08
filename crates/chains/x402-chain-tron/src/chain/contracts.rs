//! Solidity ABI bindings for the contracts TRON facilitators call into.

/// Standard ERC-20 read-only calls, used to check payer balance and allowance.
pub mod erc20 {
    use alloy_sol_types::sol;

    sol! {
        function balanceOf(address account) external view returns (uint256);
        function allowance(address owner, address spender) external view returns (uint256);
    }
}

/// EIP-3009 `transferWithAuthorization` calls, for the EIP-3009 transfer method.
pub mod eip3009 {
    use alloy_sol_types::sol;

    sol! {
        function authorizationState(address authorizer, bytes32 nonce) external view returns (bool);
        function transferWithAuthorization(
            address from,
            address to,
            uint256 value,
            uint256 validAfter,
            uint256 validBefore,
            bytes32 nonce,
            bytes calldata signature
        ) external;
    }
}

/// `x402ExactPermit2Proxy.settle` and its argument structs, for the Permit2 transfer method of x402 protocol.
pub mod x402_exact_permit2_proxy {
    use alloy_sol_types::sol;

    sol! {
        struct TronTokenPermissions {
            address token;
            uint256 amount;
        }

        struct TronPermitTransferFrom {
            TronTokenPermissions permitted;
            uint256 nonce;
            uint256 deadline;
        }

        struct TronWitness {
            address to;
            uint256 validAfter;
        }

        function settle(
            TronPermitTransferFrom permit,
            address owner,
            TronWitness witness,
            bytes signature
        ) external;
    }
}
