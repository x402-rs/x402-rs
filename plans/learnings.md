# Learnings for AI Agents Working on x402-rs

This document captures learnings from development sessions to help future AI agents understand the codebase and make better decisions.

---

## x402 Protocol: V1 vs V2 Differences

Understanding these differences is essential when working on payment-related code.

### Transport Layer

| Aspect | V1 | V2 |
|--------|----|----|
| Payment Required Location | Response body (JSON) | `Payment-Required` header (base64) |
| Payment Header Name | `X-Payment` | `Payment-Signature` |
| Network Format | Network name (e.g., "base-sepolia") | Chain ID (e.g., "eip155:84532") |

### Payload Structure

**V1** uses flat top-level fields:
```json
{
  "x402Version": 1,
  "scheme": "exact",
  "network": "base-sepolia",
  "payload": { ... }
}
```

**V2** embeds accepted requirements and resource info:
```json
{
  "x402Version": 2,
  "accepted": {
    "scheme": "exact",
    "network": "eip155:84532",
    "amount": "1000000",
    "payTo": "0x...",
    ...
  },
  "resource": {
    "description": "...",
    "mimeType": "application/json",
    "url": "..."
  },
  "payload": { ... }
}
```

### Key Implementation Points

1. **Client-side detection**: Check `Payment-Required` header first (V2), fall back to response body (V1)
2. **Resource info location**: V1 embeds in `PaymentRequirements`, V2 keeps separate in `resource` field
3. **Version detection**: Use `x402Version` field in payloads

---

## Rust Design Patterns in This Codebase

### Trait-on-Type Pattern

When creating abstractions over multiple types, prefer implementing traits directly on existing types rather than creating marker structs.

**Prefer this:**
```rust
impl PaygateProtocol for V1PriceTag { ... }
impl PaygateProtocol for v2::PaymentRequirements { ... }
```

**Over this:**
```rust
struct V1Protocol;
struct V2Protocol;
impl PaygateProtocol for V1Protocol { ... }
impl PaygateProtocol for V2Protocol { ... }
```

The first approach:
- Eliminates indirection
- Makes generics more intuitive (`Paygate<V1PriceTag>` vs `Paygate<V1Protocol>`)
- Leverages existing type semantics

### Associated Types for Version-Specific Data

When protocol versions have different payload structures, use associated types:

```rust
pub trait PaygateProtocol {
    type PaymentPayload: serde::de::DeserializeOwned + Send;
    // ...
}
```

This allows unified code to be generic while each implementation specifies its concrete type.

### Static Methods for Collection Operations

When trait methods operate on collections of the implementing type, make them static:

```rust
fn make_verify_request(
    payload: Self::PaymentPayload,
    accepts: &[Self],  // Collection of price tags
    resource: &v2::ResourceInfo,
) -> Result<proto::VerifyRequest, VerificationError>;
```

This avoids needing a separate "protocol instance" when you already have a collection of price tags.

---

## Refactoring Guidelines

### Staged Approach

When unifying duplicated code:

1. **Create new unified file** (e.g., `paygate_uni.rs`) alongside existing files
2. **Update consumers** to use the new implementation
3. **Verify compilation** at each step
4. **Delete old files** only after verification
5. **Rename** to final name

This is safer than in-place modification because each stage is independently verifiable.

### Code Sharing Estimation

When analyzing duplicate code for unification:
- Identify shared logic (HTTP handling, error handling, settlement flow)
- Identify version-specific logic (header names, payload parsing, response formatting)
- Expect 70-80% code sharing for protocol version abstractions

### Planning Documents

Create planning documents before major refactors:
- Clarifies scope
- Enables discussion of options
- Provides reference during implementation
- Documents decisions for future reference

---

## Development Process

### Incremental Verification

Run `cargo check` after each significant change:
- Catches type errors early
- Confirms trait implementations are correct
- Validates that consumers still compile

### Feedback Loops

Present design options early and get feedback:
- First solution is often not the best
- Domain experts (humans) catch unnecessary complexity
- Simpler abstractions usually win

### Domain Knowledge Emergence

Some requirements only become clear during implementation:
- The need to pass `resource` to `make_verify_request` wasn't obvious from initial analysis
- It emerged when implementing V1's verify request construction
- Be ready to adapt the design as you learn

---

## Key Files Reference

### Protocol Definitions
- `src/proto/v1.rs` - V1 protocol types
- `src/proto/v2.rs` - V2 protocol types
- `src/proto/mod.rs` - Unified protocol enums

### Server-side (Axum Example)
- `examples/x402-axum-example/src/x402/paygate.rs` - Unified paygate with `PaygateProtocol` trait
- `examples/x402-axum-example/src/x402/middleware.rs` - Axum middleware integration

### Client-side (Reqwest)
- `crates/x402-reqwest/src/http_transport.rs` - Protocol detection from responses
- `crates/x402-reqwest/src/client.rs` - Header selection based on protocol version

### Scheme Implementations
- `src/scheme/v1_eip155_exact/` - V1 EIP-155 exact payment scheme
- `src/scheme/v2_eip155_exact/` - V2 EIP-155 exact payment scheme

---

## Summary

When working on this codebase:

1. **Understand V1 vs V2 differences** - they affect header names, payload structure, and response format
2. **Prefer simple abstractions** - implement traits on existing types, use associated types for variations
3. **Refactor in stages** - create new, verify, delete old, rename
4. **Verify incrementally** - `cargo check` after each change
5. **Adapt as you learn** - some design decisions emerge during implementation
