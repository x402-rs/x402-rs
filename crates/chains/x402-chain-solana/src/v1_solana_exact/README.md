# V1 Solana Exact Scheme

This module implements the `exact` payment scheme for Solana in the x402 protocol. It verifies and settles SPL Token transfers using the `TransferChecked` instruction.

## Overview

The V1 Solana Exact scheme validates that a transaction contains a valid token transfer to the expected recipient with the correct amount. It supports both SPL Token and Token-2022 programs.

## Transaction Structure

A valid transaction must have the following structure:

| Index | Instruction                                 | Required |
|-------|---------------------------------------------|----------|
| 0     | `SetComputeUnitLimit`                       | Yes      |
| 1     | `SetComputeUnitPrice`                       | Yes      |
| 2     | `TransferChecked` (SPL Token or Token-2022) | Yes      |
| 3+    | Additional instructions (configurable)      | Optional |

## Flexible Instruction Verification

### Background

Third-party wallets like Phantom inject additional instructions into transactions for user protection. On Solana mainnet, Phantom adds the **Lighthouse program** (`L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95`) which provides security assertions.

This caused payment failures because the original implementation strictly required exactly 3 instructions.

**Related Issues:**
- [coinbase/x402#828](https://github.com/coinbase/x402/issues/828) - Solana mainnet payments failing with Phantom wallet
- [coinbase/x402#646](https://github.com/coinbase/x402/issues/646) - Comprehensive proposal for outcome-based verification
- [coinbase/x402#829](https://github.com/coinbase/x402/pull/829) - TypeScript implementation of flexible facilitator instructions

### Solution

The scheme now supports configurable instruction verification through `V1SolanaExactFacilitatorConfig`:

```rust
pub struct V1SolanaExactFacilitatorConfig {
    /// Allow additional instructions beyond the required 3
    /// Default: true
    pub allow_additional_instructions: bool,

    /// Maximum number of instructions allowed
    /// Default: 10
    pub max_instruction_count: usize,

    /// Explicitly allowed program IDs for additional instructions
    /// Default: [Phantom Lighthouse program]
    pub allowed_program_ids: Vec<Address>,

    /// Blocked program IDs (takes precedence over allowed)
    /// Default: []
    pub blocked_program_ids: Vec<Address>,

    /// Require fee payer is NOT in any instruction's accounts
    /// Default: true
    pub require_fee_payer_not_in_instructions: bool,
}
```

### Default Behavior

By default, the scheme:
- **Allows** additional instructions (to support Phantom and other wallets)
- **Whitelists** the Phantom Lighthouse program (`L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95`)
- **Limits** transactions to 10 instructions maximum
- **Requires** the fee payer to not appear in any instruction's accounts (security)

This means Phantom wallet transactions work **out of the box** without any configuration.

### Configuration

To customize the behavior, provide a config when building the facilitator:

```json
{
  "schemes": [
    {
      "id": "v1-solana-exact",
      "chains": "solana:*",
      "config": {
        "allowAdditionalInstructions": true,
        "maxInstructionCount": 10,
        "allowedProgramIds": [
          "L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95",
          "AnotherProgramIdHere"
        ],
        "blockedProgramIds": [],
        "requireFeePayerNotInInstructions": true
      }
    }
  ]
}
```

### Strict Mode

To disable additional instructions and enforce the original 3-instruction limit:

```json
{
  "config": {
    "allowAdditionalInstructions": false,
    "maxInstructionCount": 3
  }
}
```

## Security Model

### Fee Payer Protection

The facilitator's fee payer signs the transaction to pay for gas. To prevent exploitation:

1. **Fee payer isolation**: The fee payer must NOT appear in any instruction's accounts (configurable via `require_fee_payer_not_in_instructions`)
2. **Fee payer not authority**: The fee payer cannot be the transfer authority
3. **Compute budget limits**: Maximum compute unit limit and price are enforced

### Program Allowlist

When `allow_additional_instructions` is true:
- Only programs in `allowed_program_ids` are permitted
- Programs in `blocked_program_ids` are always rejected (takes precedence)
- If `allowed_program_ids` is empty, NO additional programs are allowed

### Verification Steps

1. **Decode transaction** from base64
2. **Verify compute instructions** at indices 0 and 1
3. **Validate instruction structure** (count, allowed programs)
4. **Verify TransferChecked** at index 2:
   - Correct token program (SPL Token or Token-2022)
   - Correct mint (asset)
   - Correct destination (ATA derived from pay_to + asset)
   - Correct amount
5. **Fee payer safety check** (if enabled)
6. **Simulate transaction** to verify it will succeed

## CreateATA Not Supported

The scheme does **not** support creating Associated Token Accounts (ATAs) in the payment transaction. The destination ATA must exist before the payment is submitted.

**Rationale**: Creating ATAs on-the-fly adds complexity and potential attack vectors. The recipient should ensure their ATA exists before requesting payment.

## Error Types

| Error | Description |
|-------|-------------|
| `TooFewInstructions` | Transaction has fewer than 3 instructions |
| `AdditionalInstructionsNotAllowed` | Extra instructions when `allow_additional_instructions` is false |
| `InstructionCountExceedsMax` | Transaction exceeds `max_instruction_count` |
| `BlockedProgram` | Instruction uses a blocked program |
| `ProgramNotAllowed` | Instruction uses a program not in the allowed list |
| `CreateATANotSupported` | Transaction contains CreateATA instruction |
| `FeePayerIncludedInInstructionAccounts` | Fee payer found in instruction accounts |
| `FeePayerTransferringFunds` | Fee payer is the transfer authority |
| `AssetMismatch` | Mint doesn't match expected asset |
| `RecipientMismatch` | Destination doesn't match expected ATA |
| `InvalidPaymentAmount` | Transfer amount doesn't match requirement |

## V2 Scheme

The V2 Solana Exact scheme (`v2_solana_exact`) uses the same verification logic and configuration. It differs only in the protocol message format (V2 vs V1).

## References

- [x402 Protocol Specification](https://github.com/coinbase/x402)
- [Issue #828: Solana mainnet payments failing](https://github.com/coinbase/x402/issues/828)
- [Issue #646: Outcome-based verification proposal](https://github.com/coinbase/x402/issues/646)
- [PR #829: Flexible facilitator instructions (TypeScript)](https://github.com/coinbase/x402/pull/829)
- [Lighthouse by Phantom](https://www.lighthouse.voyage) - Phantom's transaction security program
- [Lighthouse Program on Solscan](https://solscan.io/account/L2TExMFKdjpN9kozasaurPirfHy9P8sbXoAN1qA3S95)
