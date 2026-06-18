---
Document Type: Scheme
Description: Experimental - Additions to scheme_exact.md for TRON critical validation requirements (pending upstream merge)
Source PR: https://github.com/x402-foundation/x402/pull/2076
Downloaded At: 2026-06-16
Experimental: true
Note: DELETE this file once PR 2076 lands in upstream x402-foundation/x402 and scheme_exact.md is updated via the normal update process. The content below should then appear in scheme_exact.md under the Critical Validation Requirements appendix.
---

# Additions to `scheme_exact.md` — TRON (from PR #2076)

The following content is pending addition to [`scheme_exact.md`](./scheme_exact.md) in the `## Appendix > ## Critical Validation Requirements` section, alongside the existing SVM and Stellar entries.

---

### TRON

The TRON spec is byte-compatible with `scheme_exact_evm.md` at the payload-schema and EIP-712 layers — same `authorization` / `permit2Authorization` / `witness` structures, same typehashes. TRON-specific rules:

- Address conversion: Base58 addresses in `paymentRequirements` MUST be converted to EVM hex (`0x`) before signature verification and on-chain calls.
- Facilitator safety: the facilitator's address MUST NOT appear as `from` (eip3009) or `permit2Authorization.from` (permit2) in the signed payload.
- Recipient correctness: `authorization.to` (eip3009) or `witness.to` (permit2) MUST match `payTo` after Base58→hex conversion.
- Amount exactness: `authorization.value` / `permit2Authorization.permitted.amount` MUST equal `requirements.amount`.
- Signature verification: `ecrecover` only — TRON does not support ERC-1271 contract signatures.
- ERC-7710 (delegation) is not supported on TRON.
- Approval sponsoring is **not** available — TRC-20's `approve()` requires `msg.sender` to be the token owner. The `permit2` path supports only two approval paths: direct user `approve` or EIP-2612 `permit` (if the token supports it).
- The `eip3009` path uses the same on-chain interface as EIP-3009; TIP-712 (`Final`) is the signing standard. No formal TIP-3009 is required.

Network-specific rules are in per-network documents: `scheme_exact_svm.md` (Solana), `scheme_exact_stellar.md` (Stellar), `scheme_exact_evm.md` (EVM), `scheme_exact_sui.md` (SUI), `scheme_exact_tron.md` (TRON).
