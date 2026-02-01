# V2 EIP-155 Upto Payment Scheme

## Overview

The **upto scheme** is a batched payment system for EVM chains that uses EIP-2612 permits to enable multiple payments under a single authorization. Unlike the "exact" scheme which settles each payment immediately via ERC-3009 `transferWithAuthorization`, the upto scheme allows users to pre-authorize a spending cap, enabling gas-efficient batched settlements.

## Key Features

- **Batched Payments**: Multiple payments can be made under a single permit signature
- **Gas Efficient**: Reduces on-chain transactions by batching settlements
- **EIP-2612 Based**: Uses standard `permit()` function for gasless approvals
- **Session Tracking**: Server-side tracking of accumulated spending under each permit
- **Flexible Caps**: Users authorize a maximum spending amount, actual charges can be less

## Architecture

### Components

1. **Facilitator** (`src/scheme/v2_eip155_upto/mod.rs`)
   - Verifies permit signatures (EOA and EIP-1271 smart wallets)
   - Settles payments on-chain using `permit()` + `transferFrom()`
   - Handles nonce validation and allowance checking

2. **Type Definitions** (`src/scheme/v2_eip155_upto/types.rs`)
   - `UptoEvmPayload`: Contains permit signature and authorization data
   - `UptoEvmAuthorization`: EIP-2612 permit parameters (from, to, value, nonce, deadline)
   - `PaymentRequirementsExtra`: EIP-712 domain info (name, version, maxAmountRequired)

3. **Scheme Registration** (`src/scheme/mod.rs`)
   - Registered as `v2-eip155-upto` in the scheme registry
   - Supports all EVM chains (eip155:*)

## Payment Flow

### 1. Client Creates Permit

```
User → Signs EIP-2612 Permit
  ├─ owner: User's address
  ├─ spender: Facilitator's address
  ├─ value: Maximum spending cap (e.g., 10 USDC)
  ├─ nonce: Current token nonce
  └─ deadline: Expiration timestamp
```

### 2. Verification

The facilitator verifies:
- ✅ Permit signature is valid (EOA or EIP-1271)
- ✅ Spender matches one of facilitator's signers
- ✅ Cap covers required amount
- ✅ Deadline is valid (with 6 second buffer)
- ✅ Nonce matches on-chain OR sufficient allowance exists (batched payment)

### 3. Settlement

```
Facilitator → On-chain Settlement
  ├─ Step 1: Call permit(owner, spender, cap, deadline, signature)
  │   └─ Grants allowance to facilitator
  ├─ Step 2: Call transferFrom(owner, payTo, amount)
  │   └─ Transfers actual payment amount
  └─ Result: Transaction hash returned
```

### 4. Batched Payments

For subsequent payments under the same permit:
- Nonce will be different (already used)
- Facilitator checks existing allowance
- If allowance ≥ required amount, proceeds with `transferFrom`
- No need to call `permit()` again

## Implementation Details

### Signature Support

#### EOA Signatures (65 bytes)
```rust
// Standard ECDSA signature: r, s, v
permit_1(owner, spender, value, deadline, v, r, s)
```

#### EIP-1271 Smart Wallet Signatures
```rust
// Arbitrary length signature bytes
permit_0(owner, spender, value, deadline, signature)
```

#### EIP-6492 (Not Supported)
Counterfactual signatures are **explicitly rejected** with a clear error message. Users must deploy their smart wallet before using the upto scheme.

### Nonce Handling

The scheme handles two scenarios:

1. **Fresh Permit** (nonce matches on-chain)
   - First payment with this permit
   - Verification passes, settlement calls `permit()` then `transferFrom()`

2. **Reused Permit** (nonce mismatch)
   - Subsequent payment with same permit
   - Checks if existing allowance ≥ required amount
   - If yes: proceeds with `transferFrom()` only
   - If no: rejects payment

### Deadline Validation

- Deadline must be at least 6 seconds in the future
- This buffer accounts for block time and transaction propagation
- Prevents race conditions where permit expires during settlement

### Allowance Fallback

If `permit()` fails (reverts or transport error):
1. Check current allowance on-chain
2. If allowance ≥ required amount: proceed with `transferFrom()`
3. If allowance < required amount: fail with `PermitFailed` error

This handles cases where:
- Permit was already applied in a previous transaction
- Multiple payments are being processed concurrently
- Network issues caused permit transaction to fail but allowance exists

## Configuration

### Facilitator Setup

Add to `config.json`:
```json
{
  "id": "v2-eip155-upto",
  "chains": "eip155:*",
  "enabled": true
}
```

### Critical: Single Signer Requirement

⚠️ **IMPORTANT**: The facilitator MUST be configured with **only ONE signer** for the upto scheme.

**Why?**
- The permit's `spender` address must match the address that calls `transferFrom()`
- The provider uses round-robin signer selection
- With multiple signers, settlement may use a different signer than the one in the permit
- This causes `transferFrom()` to fail with "insufficient allowance"

**Correct Configuration:**
```bash
# Single signer
FACILITATOR_PRIVATE_KEY=0x...
```

**Incorrect Configuration:**
```bash
# Multiple signers - DO NOT USE with upto scheme
FACILITATOR_PRIVATE_KEY=0x...,0x...,0x...
```

## Payment Requirements

### Required Fields

```typescript
{
  scheme: "upto",
  network: "eip155:84532",  // Chain ID
  amount: "100000",          // Required amount (e.g., 0.1 USDC)
  asset: "0x...",           // Token contract address
  pay_to: "0x...",          // Recipient address
  max_timeout_seconds: 300,
  extra: {
    name: "USD Coin",       // EIP-712 domain name
    version: "2",           // EIP-712 domain version
    max_amount_required: "1000000"  // Optional: max cap requirement
  }
}
```

### Optional: Max Amount Required

The `max_amount_required` field enforces a minimum cap:
- If specified, permit cap must be ≥ `max_amount_required`
- Useful for services that want to ensure sufficient balance for multiple operations
- Example: API requiring $1 cap for batch of 10 requests at $0.10 each

## Error Handling

### Verification Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `ChainIdMismatch` | Payload/requirements chain ≠ facilitator chain | Use correct chain ID |
| `RecipientMismatch` | Spender not in facilitator's signer list | Check facilitator address |
| `InvalidPaymentAmount` | Cap < required amount | Increase permit cap |
| `Expired` | Deadline < now + 6 seconds | Use future deadline |
| `InvalidSignature` | Signature verification failed | Check signature format |
| `InvalidFormat` | Missing EIP-712 domain info | Include name/version in extra |

### Settlement Errors

| Error | Cause | Solution |
|-------|-------|----------|
| `PermitFailed` | Permit reverted & insufficient allowance | Check token balance, nonce |
| `TransferFailed` | TransferFrom reverted | Check allowance, balance |
| `InvalidSignature` | EIP-6492 signature detected | Deploy wallet first |

## Testing

The implementation includes 37 comprehensive tests covering:

### Scheme Blueprint Tests
- Scheme ID validation (`v2-eip155-upto`)
- Builder rejects non-EIP155 providers
- Facilitator creation

### Verification Tests
- Valid permit signatures (EOA and EIP-1271)
- Chain ID validation (payload and requirements)
- Spender validation (single and multiple facilitator addresses)
- Cap validation (too low, exact, sufficient)
- Max amount required validation
- Deadline validation (expired, too soon, valid, boundary)
- Invalid signatures
- Missing EIP-712 domain info
- Nonce mismatch with sufficient/insufficient allowance
- EIP-6492 rejection

### Settlement Tests
- Successful permit and transfer
- Permit reverts with sufficient allowance
- Permit reverts with insufficient allowance
- Permit transport error with sufficient allowance
- Permit transport error with insufficient allowance
- Transfer fails
- Invalid signature format
- EIP-6492 rejection

### Facilitator Tests
- Verify endpoint
- Settle endpoint
- Supported endpoint

### Edge Cases
- Zero amount payments
- Maximum U256 cap
- Zero nonce
- Maximum U256 nonce
- Empty facilitator addresses

### Type Tests
- Serialization/deserialization
- CamelCase conversion
- Optional field handling
- Roundtrip encoding

## API Reference

### Verify Request

```json
{
  "x402_version": 2,
  "payment_payload": {
    "accepted": { /* payment requirements */ },
    "payload": {
      "signature": "0x...",
      "authorization": {
        "from": "0x...",
        "to": "0x...",
        "value": "0x...",
        "nonce": "0x...",
        "validBefore": "0x..."
      }
    }
  },
  "payment_requirements": { /* same as accepted */ }
}
```

### Verify Response

```json
{
  "success": true,
  "payer": "0x..."
}
```

### Settle Request

Same as verify request.

### Settle Response

```json
{
  "success": true,
  "payer": "0x...",
  "transaction": "0x...",
  "network": "eip155:84532"
}
```

### Supported Response

```json
{
  "kinds": [{
    "x402_version": 2,
    "scheme": "upto",
    "network": "eip155:84532",
    "extra": null
  }],
  "extensions": [],
  "signers": {
    "eip155:84532": ["0x..."]
  }
}
```

## Comparison: Upto vs Exact

| Feature | Upto Scheme | Exact Scheme |
|---------|-------------|--------------|
| **Authorization** | EIP-2612 permit | ERC-3009 transferWithAuthorization |
| **Batching** | ✅ Multiple payments per permit | ❌ One payment per signature |
| **Gas Efficiency** | High (batch settlements) | Lower (per-payment settlement) |
| **Nonce Management** | Token nonce | Authorization nonce |
| **Smart Wallets** | ✅ EIP-1271 supported | ✅ EIP-1271 + EIP-6492 supported |
| **Counterfactual** | ❌ Not supported | ✅ EIP-6492 supported |
| **Use Case** | Frequent small payments | One-time larger payments |
| **Settlement** | permit() + transferFrom() | transferWithAuthorization() |

## Best Practices

### For Facilitators

1. **Use Single Signer**: Configure only one signer address for upto scheme
2. **Monitor Allowances**: Track permit usage to detect potential issues
3. **Handle Nonce Mismatches**: Implement allowance fallback logic
4. **Set Reasonable Deadlines**: Give users enough time (e.g., 5 minutes)
5. **Validate Domain Info**: Ensure name/version match token contract

### For Clients

1. **Set Appropriate Caps**: Balance between security and convenience
2. **Monitor Spending**: Track accumulated spending under each permit
3. **Refresh Permits**: Create new permits when cap is exhausted
4. **Handle Expiration**: Renew permits before deadline
5. **Check Nonces**: Fetch current nonce before signing

### For Resource Servers

1. **Track Sessions**: Implement session store for batched payments
2. **Settle Periodically**: Use sweeper for automatic batch settlement
3. **Set Thresholds**: Define when to trigger settlement (amount or time)
4. **Handle Failures**: Implement retry logic for failed settlements
5. **Monitor Balances**: Ensure users have sufficient token balance

## Security Considerations

### Signature Validation

- EOA signatures verified via ecrecover
- EIP-1271 signatures verified via contract call
- EIP-6492 explicitly rejected (deploy wallet first)
- All signatures checked against expected signer

### Replay Protection

- Nonce prevents replay attacks
- Deadline prevents stale permits
- Chain ID prevents cross-chain replay
- Verifying contract address in EIP-712 domain

### Allowance Management

- Facilitator can only spend up to approved amount
- Each transferFrom reduces allowance
- Users can revoke allowance anytime via token contract
- Permit expiration limits exposure window

### Multi-Signer Risk

- Round-robin selection can cause settlement failures
- Single signer configuration required
- Document this limitation clearly
- Consider implementing signer pinning in future

## Future Enhancements

### Potential Improvements

1. **Signer Pinning**: Allow specifying which signer to use for settlement
2. **Batch Settlement API**: Endpoint to settle multiple payments at once
3. **Permit Refresh**: Automatic permit renewal before expiration
4. **EIP-6492 Support**: Enable counterfactual wallet signatures
5. **Multi-Token**: Support multiple tokens in single permit
6. **Gasless Relaying**: Meta-transaction support for permit submission

### Integration Opportunities

1. **Session Management**: Built-in session store for resource servers
2. **Auto-Sweeper**: Configurable automatic settlement triggers
3. **Analytics**: Track permit usage, settlement patterns
4. **Rate Limiting**: Per-permit spending rate limits
5. **Webhooks**: Notify on settlement events

## Troubleshooting

### Common Issues

**Issue**: Settlement fails with "insufficient allowance"
- **Cause**: Multiple signers configured
- **Solution**: Use single signer configuration

**Issue**: Verification fails with "nonce mismatch"
- **Cause**: Permit already used, insufficient allowance
- **Solution**: Check on-chain allowance, may need new permit

**Issue**: "EIP-6492 not supported" error
- **Cause**: Using counterfactual wallet signature
- **Solution**: Deploy wallet before using upto scheme

**Issue**: "Expired" error despite future deadline
- **Cause**: Deadline < now + 6 seconds
- **Solution**: Use deadline at least 10 seconds in future

**Issue**: Permit succeeds but transfer fails
- **Cause**: Insufficient token balance
- **Solution**: Ensure user has enough tokens

## Technical Deep Dive: Upto Transaction Flow

This section provides a comprehensive technical explanation of what happens when an upto payment is made, including all component interactions, data transformations, and on-chain operations.

### System Architecture Overview

```
┌─────────────┐         ┌──────────────────┐         ┌─────────────────┐         ┌──────────────┐
│   Client    │────────▶│ Resource Server  │────────▶│   Facilitator   │────────▶│  Blockchain │
│ (TypeScript)│         │  (TypeScript)    │         │     (Rust)      │         │   (Polygon) │
└─────────────┘         └──────────────────┘         └─────────────────┘         └──────────────┘
     │                           │                            │                          │
     │                           │                            │                          │
     │ 1. Create Permit          │                            │                          │
     │ 2. Sign EIP-712           │                            │                          │
     │ 3. Cache Permit           │                            │                          │
     │ 4. Send Payment Header     │                            │                          │
     │                           │ 5. Extract Payment         │                          │
     │                           │ 6. Call /verify            │                          │
     │                           │                            │ 7. Verify Signature      │
     │                           │                            │ 8. Check Nonce/Allowance│
     │                           │                            │ 9. Return Success       │
     │                           │ 10. Track Payment           │                          │
     │                           │ 11. Return Content         │                          │
     │                           │                            │                          │
     │                           │ [Later: Settlement]        │                          │
     │                           │                            │ 12. Call /settle         │
     │                           │                            │ 13. permit() on-chain    │
     │                           │                            │ 14. transferFrom()      │
     │                           │                            │                          │
     │                           │                            │                          │ 15. Transaction
     │                           │                            │                          │    Confirmed
```

### Phase 1: Client-Side Permit Creation

#### Step 1.1: Initial Request (No Payment)

**Client Action:**
```typescript
const response = await fetch('https://api.example.com/protected');
// Response: 402 Payment Required
```

**Resource Server Response:**
```json
{
  "x402Version": 2,
  "error": "Payment required",
  "accepts": [{
    "scheme": "upto",
    "network": "eip155:137",
    "amount": "1000",  // 0.001 USDC (6 decimals)
    "asset": "0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359",
    "payTo": "0x92Adc157197045A367cFCCBFaE206E93eBF8E38A",
    "maxTimeoutSeconds": 300,
    "extra": {
      "name": "USD Coin",
      "version": "2"
    }
  }]
}
```

**Technical Details:**
- Server returns `402 Payment Required` status code
- Payment requirements encoded in `Payment-Required` header (base64 JSON)
- Client parses requirements to understand what payment is needed

#### Step 1.2: Fetch Facilitator Signer Address

**Client Action:**
```typescript
const supported = await fetch('http://localhost:8090/supported');
const signers = supported.signers['eip155:*'];
const facilitatorAddress = signers[0]; // 0xBBc4344Bb405858959d81aB1DEadD7a13EC37E13
```

**Facilitator Response:**
```json
{
  "kinds": [{
    "x402Version": 2,
    "scheme": "upto",
    "network": "eip155:137"
  }],
  "signers": {
    "eip155:*": ["0xBBc4344Bb405858959d81aB1DEadD7a13EC37E13"]
  }
}
```

**Technical Details:**
- Client needs facilitator's address to set as `spender` in permit
- Facilitator exposes signer addresses via `/supported` endpoint
- Client uses first signer (critical: must match settlement signer)

#### Step 1.3: Check Permit Cache

**Client Logic:**
```typescript
const cacheKey = `${assetAddress}-${facilitatorAddress}`;
let permit = permitCache.get(cacheKey);

if (!permit) {
  // Create new permit
} else {
  // Reuse cached permit (batched payment)
}
```

**Technical Details:**
- Permits are cached by `(token, spender)` tuple
- Same permit can be reused for multiple payments
- Cache persists until permit expires or cap exhausted

#### Step 1.4: Fetch Current Token Nonce

**Client Action:**
```typescript
const tokenContract = new ethers.Contract(
  assetAddress,
  ['function nonces(address owner) view returns (uint256)'],
  provider
);
const nonce = await tokenContract.nonces(wallet.address);
// Result: 3 (current nonce on-chain)
```

**On-Chain Call:**
```
eth_call(
  to: 0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359,
  data: 0x7ecebe00... (nonces(address) selector + wallet address)
)
```

**Technical Details:**
- EIP-2612 tokens maintain a nonce per owner
- Nonce increments after each `permit()` call
- Client must use current nonce for signature validity

#### Step 1.5: Calculate Permit Parameters

**Client Calculation:**
```typescript
const amount = BigInt(requirements.amount);        // 1000 (0.001 USDC)
const cap = amount * 10n;                          // 10000 (0.01 USDC cap)
const deadline = Math.floor(Date.now() / 1000) + 3600; // 1 hour from now

const permitData = {
  owner: wallet.address,                           // 0x0911B03B03bda4F7f308277768Bddc0055aAE66b
  spender: facilitatorAddress,                     // 0xBBc4344Bb405858959d81aB1DEadD7a13EC37E13
  value: cap.toString(),                           // "10000"
  nonce: nonce.toString(),                         // "3"
  deadline: deadline.toString()                    // "1738260000"
};
```

**Technical Details:**
- Cap is typically 10x the single payment amount (allows 10 batched payments)
- Deadline set to future timestamp (Unix seconds)
- All values must match exactly what will be verified on-chain

#### Step 1.6: Construct EIP-712 Domain

**Client Construction:**
```typescript
const domain = {
  name: "USD Coin",                    // From requirements.extra.name
  version: "2",                        // From requirements.extra.version
  chainId: 137,                        // Polygon Mainnet
  verifyingContract: assetAddress      // USDC contract address
};
```

**EIP-712 Domain Hash:**
```
keccak256(
  keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)") +
  keccak256("USD Coin") +
  keccak256("2") +
  137 +
  0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359
)
```

**Technical Details:**
- EIP-712 domain uniquely identifies the signing context
- Prevents signature replay across different tokens/chains
- Must match exactly what facilitator will use for verification

#### Step 1.7: Sign Permit with EIP-712

**Client Action:**
```typescript
const PERMIT_TYPES = {
  Permit: [
    { name: 'owner', type: 'address' },
    { name: 'spender', type: 'address' },
    { name: 'value', type: 'uint256' },
    { name: 'nonce', type: 'uint256' },
    { name: 'deadline', type: 'uint256' },
  ],
};

const signature = await wallet.signTypedData(domain, PERMIT_TYPES, permitData);
// Result: 0x1234...abcd (65 bytes for EOA, variable for EIP-1271)
```

**EIP-712 Hash Calculation:**
```
structHash = keccak256(
  keccak256("Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)") +
  owner +
  spender +
  value +
  nonce +
  deadline
)

messageHash = keccak256(
  "\x19\x01" +  // EIP-712 prefix
  domainSeparator +
  structHash
)

signature = sign(messageHash, privateKey)
```

**Technical Details:**
- EIP-712 provides structured data signing (not just raw hash)
- Signature format: 65 bytes for EOA (r, s, v), variable for EIP-1271
- Signature proves user authorized specific spending cap

#### Step 1.8: Construct Payment Payload

**Client Construction:**
```typescript
const paymentPayload = {
  x402Version: 2,
  accepted: requirements,              // Requirements from 402 response
  payload: {
    signature: signature,              // EIP-712 signature
    authorization: {
      from: wallet.address,
      to: facilitatorAddress,
      value: `0x${cap.toString(16)}`,  // "0x2710" (hex)
      nonce: `0x${nonce.toString(16)}`, // "0x3" (hex)
      validBefore: `0x${deadline.toString(16)}` // "0x6791a460" (hex)
    }
  },
  resource: {
    url: "https://api.example.com/protected",
    description: "Protected API endpoint",
    mimeType: "application/json"
  }
};
```

**Technical Details:**
- `accepted` field contains the requirements user is accepting
- `payload` contains the permit signature and authorization data
- All numeric values encoded as hex strings (EVM format)

#### Step 1.9: Encode and Send Payment Header

**Client Action:**
```typescript
const paymentHeader = Buffer.from(JSON.stringify(paymentPayload)).toString('base64');
// Result: "eyJ4NDAyVmVyc2lvbiI6Miw...very long string..."

const response = await fetch('https://api.example.com/protected', {
  headers: {
    'Payment-Signature': paymentHeader
  }
});
```

**HTTP Request:**
```
GET /protected HTTP/1.1
Host: api.example.com
Payment-Signature: eyJ4NDAyVmVyc2lvbiI6Miw...
```

**Technical Details:**
- Payment payload encoded as base64 for HTTP header transmission
- Header name: `Payment-Signature` (V2 protocol)
- Server decodes and parses JSON to extract payment data

### Phase 2: Resource Server Processing

#### Step 2.1: Extract Payment Header

**Server Middleware:**
```typescript
const paymentHeader = req.headers['payment-signature'];
if (!paymentHeader) {
  return res.status(402).json({ error: 'Payment required' });
}

const paymentPayload = JSON.parse(
  Buffer.from(paymentHeader, 'base64').toString()
);
```

**Technical Details:**
- Middleware intercepts request before handler execution
- Extracts and decodes payment header
- Returns 402 if payment missing

#### Step 2.2: Call Facilitator Verify Endpoint

**Server Action:**
```typescript
const verifyRequest = {
  x402Version: 2,
  paymentPayload: paymentPayload,
  paymentRequirements: paymentPayload.accepted
};

const verifyResponse = await fetch('http://localhost:8090/verify', {
  method: 'POST',
  headers: { 'Content-Type': 'application/json' },
  body: JSON.stringify(verifyRequest)
});
```

**HTTP Request to Facilitator:**
```
POST /verify HTTP/1.1
Host: localhost:8090
Content-Type: application/json

{
  "x402Version": 2,
  "paymentPayload": { ... },
  "paymentRequirements": { ... }
}
```

**Technical Details:**
- Server forwards payment to facilitator for verification
- Facilitator is trusted third party that validates signatures
- Verification happens off-chain (no gas cost)

### Phase 3: Facilitator Verification (Rust)

#### Step 3.1: Parse Verify Request

**Facilitator Code:**
```rust
let request: types::VerifyRequest = types::VerifyRequest::from_proto(
    proto::VerifyRequest::from(json)
)?;

let payload = &request.payment_payload;
let requirements = &request.payment_requirements;
```

**Data Structures:**
```rust
pub struct PaymentPayload {
    pub accepted: PaymentRequirements,      // Requirements user accepted
    pub payload: UptoEvmPayload,            // Permit signature + auth
    pub resource: Option<ResourceInfo>,     // Resource being accessed
    pub x402_version: v2::X402Version2,
}

pub struct UptoEvmPayload {
    pub signature: Bytes,                   // EIP-712 signature
    pub authorization: UptoEvmAuthorization,
}

pub struct UptoEvmAuthorization {
    pub from: Address,                      // Token owner
    pub to: Address,                        // Facilitator (spender)
    pub value: U256,                         // Spending cap
    pub nonce: U256,                        // Token nonce
    pub valid_before: U256,                  // Deadline timestamp
}
```

**Technical Details:**
- Rust deserializes JSON into strongly-typed structs
- Type safety prevents invalid data from reaching verification logic
- `from_proto` converts protocol-agnostic format to scheme-specific types

#### Step 3.2: Validate Chain ID

**Facilitator Code:**
```rust
let chain_id: ChainId = provider.chain().into(); // eip155:137
let payload_chain_id = &payload.accepted.network;

if payload_chain_id != &chain_id {
    return Err(PaymentVerificationError::ChainIdMismatch.into());
}
```

**Technical Details:**
- Prevents cross-chain replay attacks
- Ensures permit signed for correct chain
- Chain ID embedded in EIP-712 domain

#### Step 3.3: Validate Spender Address

**Facilitator Code:**
```rust
let signer_addresses: Vec<Address> = provider
    .signer_addresses()
    .iter()
    .filter_map(|s| s.parse().ok())
    .collect();

let spender = authorization.to;
if !signer_addresses.contains(&spender) {
    return Err(PaymentVerificationError::RecipientMismatch.into());
}
```

**Technical Details:**
- Permit's `spender` must match facilitator's signer
- Critical: Only the signer address can call `transferFrom()`
- Multiple signers supported (but see single-signer requirement note)

#### Step 3.4: Validate Cap Covers Amount

**Facilitator Code:**
```rust
let cap = authorization.value;              // 10000 (0.01 USDC)
let required_amount = requirements.amount.0; // 1000 (0.001 USDC)

if cap < required_amount {
    return Err(PaymentVerificationError::InvalidPaymentAmount.into());
}

// Check max_amount_required if specified
if let Some(max_amount_required) = requirements.extra.max_amount_required {
    if cap < max_amount_required {
        return Err(PaymentVerificationError::InvalidPaymentAmount.into());
    }
}
```

**Technical Details:**
- Cap must be ≥ required amount (can be larger for batching)
- Optional `max_amount_required` enforces minimum cap
- Allows users to authorize more than needed

#### Step 3.5: Validate Deadline

**Facilitator Code:**
```rust
let now = UnixTimestamp::now();
let deadline_timestamp = UnixTimestamp::from_secs(
    deadline.to::<u64>()
);

if deadline_timestamp < now + 6 {
    return Err(PaymentVerificationError::Expired.into());
}
```

**Technical Details:**
- 6-second buffer accounts for block time and network latency
- Prevents permits expiring during settlement
- Deadline checked in seconds (Unix timestamp)

#### Step 3.6: Fetch On-Chain Nonce

**Facilitator Code:**
```rust
let token_contract = IEIP3009::new(asset_address, provider);
let on_chain_nonce: U256 = token_contract
    .nonces(owner)
    .call()
    .await?;
```

**On-Chain Call:**
```
eth_call(
  to: 0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359,
  data: 0x7ecebe00... (nonces(address) selector + owner address)
)
```

**Technical Details:**
- Reads current nonce from token contract
- Used to detect if permit is fresh or reused
- Nonce increments after each `permit()` call

#### Step 3.7: Handle Nonce Mismatch (Batched Payment)

**Facilitator Code:**
```rust
if nonce != on_chain_nonce {
    // Permit already used - check allowance
    let allowance: U256 = token_contract
        .allowance(owner, spender)
        .call()
        .await?;

    if allowance < required_amount {
        return Err(PaymentVerificationError::InvalidFormat(
            format!("Nonce mismatch and insufficient allowance")
        ).into());
    }
    
    // Allowance sufficient - permit was already applied
    tracing::info!("Nonce mismatch but sufficient allowance - accepting batched payment");
}
```

**On-Chain Call:**
```
eth_call(
  to: 0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359,
  data: 0xdd62ed3e... (allowance(address,address) selector + owner + spender)
)
```

**Technical Details:**
- Nonce mismatch indicates permit already used
- For batched payments, check existing allowance
- If allowance ≥ required amount, permit was already applied
- This enables multiple payments under same permit

#### Step 3.8: Construct EIP-712 Domain

**Facilitator Code:**
```rust
let domain = eip712_domain! {
    name: extra.name.clone(),              // "USD Coin"
    version: extra.version.clone(),        // "2"
    chain_id: chain.inner(),              // 137
    verifying_contract: asset_address,     // USDC address
};
```

**Technical Details:**
- Must match exactly what client used for signing
- Domain uniquely identifies signing context
- Prevents signature replay

#### Step 3.9: Compute EIP-712 Hash

**Facilitator Code:**
```rust
let permit = Permit {
    owner,
    spender,
    value: cap,
    nonce,
    deadline,
};

let eip712_hash = permit.eip712_signing_hash(&domain);
// Result: B256 hash that should match signed message
```

**Hash Calculation:**
```rust
// Same as client-side calculation
struct_hash = keccak256(
    keccak256("Permit(...)") +
    owner +
    spender +
    value +
    nonce +
    deadline
)

message_hash = keccak256(
    "\x19\x01" +
    domain_separator +
    struct_hash
)
```

**Technical Details:**
- Recomputes hash using same algorithm as client
- Hash must match what was signed
- Used for signature verification

#### Step 3.10: Verify Signature

**Facilitator Code:**
```rust
let signature = &payload.payload.signature;
verify_permit_signature(provider, owner, eip712_hash, signature).await?;
```

**For EOA Signatures:**
```rust
let sig = Signature::try_from(signature.as_slice())?;
let recovered_address = sig.recover_signer(eip712_hash)?;

if recovered_address != owner {
    return Err(InvalidSignature);
}
```

**For EIP-1271 Smart Wallets:**
```rust
let contract = IEIP1271::new(owner, provider);
let magic_value = contract
    .isValidSignature(eip712_hash, signature)
    .call()
    .await?;

if magic_value != [0x16, 0x26, 0xba, 0x7e] {
    return Err(InvalidSignature);
}
```

**Technical Details:**
- EOA: Recover signer from signature, compare to owner
- EIP-1271: Call contract's `isValidSignature()` method
- EIP-6492: Explicitly rejected (not supported)

#### Step 3.11: Return Verification Result

**Facilitator Response:**
```json
{
  "isValid": true,
  "payer": "0x0911B03B03bda4F7f308277768Bddc0055aAE66b"
}
```

**Technical Details:**
- Verification successful, payment is valid
- Returns payer address for tracking
- No on-chain transaction yet (verification is off-chain)

### Phase 4: Resource Server Response

#### Step 4.1: Track Payment (No Settlement)

**Server Code:**
```typescript
if (verifyResult.isValid) {
    // For upto scheme: DO NOT SETTLE immediately
    // Track payment for later batch settlement
    
    const sessionKey = `${payer}-upto`;
    const session = paymentSessions.get(sessionKey) || {
        payer: payer,
        payments: [],
        totalAmount: 0n
    };
    
    session.payments.push({
        amount: '1000',
        timestamp: Date.now(),
        paymentPayload: paymentPayload,
        requirements: requirements
    });
    session.totalAmount += 1000n;
    
    paymentSessions.set(sessionKey, session);
    
    // Continue to handler
    next();
}
```

**Technical Details:**
- Upto scheme: Payments tracked but NOT settled
- Session store accumulates payments per payer
- Settlement happens separately (manual or automatic)

#### Step 4.2: Return Protected Content

**Server Response:**
```json
{
  "message": "🎉 Protected content accessed successfully!",
  "scheme": "upto",
  "note": "Payment verified but not settled yet (batching)"
}
```

**HTTP Response:**
```
HTTP/1.1 200 OK
Content-Type: application/json

{ "message": "..." }
```

**Technical Details:**
- Content returned after verification
- No settlement transaction yet
- Payment tracked for batch settlement

### Phase 5: Settlement (Later, Separate Trigger)

#### Step 5.1: Settlement Trigger

**Trigger Options:**
1. **Manual**: `POST /settle/:payer` endpoint called
2. **Automatic**: Sweeper runs periodically
3. **Threshold**: Accumulated amount reaches limit
4. **Session End**: User logs out or session expires

**Example Manual Trigger:**
```typescript
const settleResponse = await fetch('http://localhost:3000/settle/0x0911...', {
    method: 'POST'
});
```

#### Step 5.2: Call Facilitator Settle Endpoint

**Server Action:**
```typescript
const settleRequest = {
    x402Version: 2,
    paymentPayload: lastPayment.paymentPayload,
    paymentRequirements: lastPayment.requirements
};

const settleResponse = await fetch('http://localhost:8090/settle', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify(settleRequest)
});
```

**Technical Details:**
- Uses same payload as verification
- Facilitator will verify again, then settle on-chain
- Settlement is idempotent (can be retried)

#### Step 5.3: Facilitator Re-Verification

**Facilitator Code:**
```rust
// Same verification as Step 3
let payer = verify_upto_payment(...).await?;
```

**Technical Details:**
- Re-verifies payment before settling
- Prevents invalid settlements
- Ensures payment still valid

#### Step 5.4: Determine Permit Overload

**Facilitator Code:**
```rust
let structured_sig = StructuredSignature::try_from_bytes(
    signature.clone(),
    owner,
    &eip712_hash
)?;

let permit_calldata = match structured_sig {
    StructuredSignature::EOA(sig) => {
        // Use permit_1(owner, spender, value, deadline, v, r, s)
        let v = if sig.v() { 28u8 } else { 27u8 };
        token_contract
            .permit_1(owner, spender, cap, deadline, v, sig.r().into(), sig.s().into())
            .calldata()
    }
    StructuredSignature::EIP1271(_) => {
        // Use permit_0(owner, spender, value, deadline, bytes signature)
        token_contract
            .permit_0(owner, spender, cap, deadline, signature.clone())
            .calldata()
    }
};
```

**Technical Details:**
- EIP-2612 has two permit overloads:
  - `permit_1`: For EOA signatures (r, s, v)
  - `permit_0`: For arbitrary signatures (bytes)
- Must use correct overload based on signature type

#### Step 5.5: Execute Permit Transaction

**Facilitator Code:**
```rust
let permit_result = provider.send_transaction(MetaTransaction {
    to: asset_address,
    calldata: permit_calldata,
    confirmations: 1,
}).await;
```

**On-Chain Transaction:**
```
Transaction {
    to: 0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359,
    data: 0xd505accf... (permit_1 or permit_0 calldata),
    from: 0xBBc4344Bb405858959d81aB1DEadD7a13EC37E13 (facilitator signer)
}
```

**On-Chain Execution:**
```solidity
function permit_1(
    address owner,
    address spender,
    uint256 value,
    uint256 deadline,
    uint8 v,
    bytes32 r,
    bytes32 s
) external {
    require(deadline >= block.timestamp, "Permit expired");
    require(owner != address(0), "Invalid owner");
    
    bytes32 structHash = keccak256(abi.encode(
        keccak256("Permit(address owner,address spender,uint256 value,uint256 nonce,uint256 deadline)"),
        owner,
        spender,
        value,
        nonces[owner],
        deadline
    ));
    
    bytes32 hash = keccak256(abi.encodePacked("\x19\x01", DOMAIN_SEPARATOR, structHash));
    address signer = ecrecover(hash, v, r, s);
    require(signer == owner, "Invalid signature");
    
    nonces[owner]++;
    _approve(owner, spender, value);
}
```

**Technical Details:**
- `permit()` grants allowance to facilitator
- Nonce increments (prevents replay)
- Allowance set to cap value (not individual payment amount)

#### Step 5.6: Handle Permit Failure (Allowance Fallback)

**Facilitator Code:**
```rust
match permit_result {
    Ok(receipt) if !receipt.status() => {
        // Permit reverted - check if allowance already exists
        let allowance: U256 = token_contract
            .allowance(owner, spender)
            .call()
            .await?;
        
        if allowance < amount {
            return Err(Eip155UptoError::PermitFailed);
        }
        // Allowance sufficient - proceed
    }
    Err(e) => {
        // Transport error - check allowance
        let allowance: U256 = token_contract
            .allowance(owner, spender)
            .call()
            .await?;
        
        if allowance < amount {
            return Err(Eip155UptoError::PermitFailed);
        }
        // Allowance sufficient - proceed
    }
    _ => {} // Permit succeeded
}
```

**Technical Details:**
- Permit may fail if already applied (nonce mismatch)
- Check existing allowance as fallback
- If allowance sufficient, skip permit and proceed to transfer

#### Step 5.7: Execute TransferFrom Transaction

**Facilitator Code:**
```rust
let transfer_calldata = token_contract
    .transferFrom(owner, pay_to, amount)
    .calldata()
    .clone();

let receipt = provider.send_transaction(MetaTransaction {
    to: asset_address,
    calldata: transfer_calldata,
    confirmations: 1,
}).await?;
```

**On-Chain Transaction:**
```
Transaction {
    to: 0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359,
    data: 0x23b872dd... (transferFrom(address,address,uint256) + owner + payTo + amount),
    from: 0xBBc4344Bb405858959d81aB1DEadD7a13EC37E13 (facilitator signer)
}
```

**On-Chain Execution:**
```solidity
function transferFrom(address from, address to, uint256 amount) external returns (bool) {
    require(from != address(0), "Invalid from");
    require(to != address(0), "Invalid to");
    
    uint256 currentAllowance = allowances[from][msg.sender];
    require(currentAllowance >= amount, "Insufficient allowance");
    
    allowances[from][msg.sender] = currentAllowance - amount;
    balances[from] -= amount;
    balances[to] += amount;
    
    emit Transfer(from, to, amount);
    return true;
}
```

**Technical Details:**
- `transferFrom()` transfers actual payment amount (not cap)
- Reduces allowance by transferred amount
- Remaining allowance available for future payments
- Transaction must be from facilitator signer (authorized spender)

#### Step 5.8: Return Settlement Result

**Facilitator Response:**
```json
{
  "success": true,
  "payer": "0x0911B03B03bda4F7f308277768Bddc0055aAE66b",
  "transaction": "0x96060d4bbbdf5a785f4f4c68c7c27ee4173da24a224320986d75ca2f46ab1ae3",
  "network": "eip155:137"
}
```

**Technical Details:**
- Returns transaction hash for blockchain explorer
- Settlement successful, payment collected on-chain
- Resource server can clear tracked payments

### Phase 6: Batched Payment Flow (Subsequent Requests)

#### Step 6.1: Client Reuses Cached Permit

**Client Action:**
```typescript
// Second API call - same permit reused
const permit = permitCache.get(cacheKey); // Still valid
// No new permit creation needed
```

**Technical Details:**
- Same permit signature sent again
- Nonce in permit is now stale (was 3, on-chain is now 4)
- Facilitator detects nonce mismatch

#### Step 6.2: Facilitator Detects Nonce Mismatch

**Facilitator Code:**
```rust
let on_chain_nonce: U256 = token_contract.nonces(owner).call().await?;
// Result: 4 (incremented after first permit)

if nonce != on_chain_nonce {
    // Permit already used - check allowance
    let allowance: U256 = token_contract.allowance(owner, spender).call().await?;
    // Result: 9000 (10000 cap - 1000 first payment = 9000 remaining)
    
    if allowance >= required_amount {
        // Sufficient allowance - accept payment
    }
}
```

**Technical Details:**
- Nonce mismatch indicates permit already applied
- Check remaining allowance from first permit
- If allowance sufficient, payment valid (batched payment)

#### Step 6.3: Verification Succeeds (No Settlement)

**Technical Details:**
- Payment verified but NOT settled
- Another payment tracked in session
- Settlement will batch all payments together

#### Step 6.4: Settlement Batches All Payments

**When Settlement Triggered:**
```typescript
// All 3 payments settled together
session.payments = [
    { amount: 1000 }, // Payment 1
    { amount: 1000 }, // Payment 2
    { amount: 1000 }  // Payment 3
];
// Total: 3000 (0.003 USDC)
```

**Settlement Transaction:**
- One `permit()` call (if needed, or skip if allowance exists)
- One `transferFrom()` call for total amount (3000)
- **Result: 1 transaction for 3 payments** (vs 3 transactions with exact scheme)

### Data Flow Summary

```
Client:
  Permit Creation → EIP-712 Signing → Payment Payload → Base64 Encoding → HTTP Header

Resource Server:
  Header Extraction → Base64 Decoding → Facilitator /verify → Track Payment → Return Content

Facilitator (Verify):
  Parse Request → Validate Chain/Spender/Cap/Deadline → Check Nonce/Allowance → 
  Recompute EIP-712 Hash → Verify Signature → Return Success

Facilitator (Settle):
  Re-Verify → Determine Permit Overload → permit() on-chain → 
  transferFrom() on-chain → Return Transaction Hash

Blockchain:
  permit() → Grant Allowance → transferFrom() → Transfer Tokens → Emit Events
```

### Key Technical Insights

1. **Off-Chain Verification**: Payment verification happens off-chain (no gas), only settlement requires on-chain transaction

2. **Batching Efficiency**: Multiple payments verified separately but settled together in one transaction

3. **Nonce Management**: First payment uses permit (nonce increments), subsequent payments check allowance

4. **Signature Flexibility**: Supports both EOA (65 bytes) and EIP-1271 (variable length) signatures

5. **Allowance Fallback**: If permit fails (already applied), check existing allowance as fallback

6. **Session Tracking**: Resource server tracks payments server-side, settlement is separate operation

7. **Gas Optimization**: Batching reduces gas costs from N transactions to 1-2 transactions (permit + transferFrom)

## References

- [EIP-2612: Permit Extension for EIP-20](https://eips.ethereum.org/EIPS/eip-2612)
- [EIP-712: Typed Structured Data Hashing](https://eips.ethereum.org/EIPS/eip-712)
- [EIP-1271: Standard Signature Validation](https://eips.ethereum.org/EIPS/eip-1271)
- [ERC-3009: Transfer With Authorization](https://eips.ethereum.org/EIPS/eip-3009)
- [x402 Protocol Specification](https://github.com/polysensus/x402)

## License

This implementation is part of the x402-rs project and follows the same license terms.
