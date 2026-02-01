# Upto Scheme Integration Test

Hybrid integration test demonstrating the `v2-eip155-upto` payment scheme using:
- **Rust facilitator** (`x402-rs`) for payment verification and settlement
- **TypeScript client** (`@daydreamsai/facilitator/client`) for permit creation
- **TypeScript Express middleware** (`@daydreamsai/facilitator/express`) for payment handling

## Quick Start

```bash
# Install dependencies
pnpm install

# Terminal 1: Start Rust facilitator
cd /Users/agada/x402-rs_Polygon
cargo run --bin x402-rs -- --config config.json

# Terminal 2: Start Express server
cd examples/upto-rust-ts-test
pnpm run server

# Terminal 3: Run test client
pnpm run client
```

## What It Tests

- ✅ **3 API calls** at $0.001 USDC each
- ✅ **Permit caching** - Single permit reused for all calls
- ✅ **Batched payments** - Payments tracked server-side, no immediate settlement
- ✅ **Threshold settlement** - Auto-settles at $0.003 USDC (3 payments)
- ✅ **Single transaction** - One settlement transaction for all batched payments

## Architecture

```
┌──────────────┐      ┌──────────────┐      ┌──────────────┐
│ TS Client   │─────▶│ Express      │─────▶│ Rust         │
│ (daydreams) │      │ Middleware   │      │ Facilitator  │
└──────────────┘      └──────────────┘      └──────────────┘
     │                      │                      │
     │ Creates permit       │ Tracks sessions      │ Verifies/Settles
     │ Caches for reuse     │ Checks threshold     │ On-chain tx
     │                      │ Auto-settles         │
```

## Transaction Flow

1. **Call #1**: Client creates EIP-2612 permit (0.01 USDC cap) → Server verifies → Tracks payment
2. **Call #2**: Client reuses permit → Server verifies → Tracks payment (no settlement)
3. **Call #3**: Client reuses permit → Server verifies → Threshold reached → Auto-settles

**Result**: 3 API calls → 2 on-chain transactions (1 permit + 1 settlement)

## Configuration

See `.env` for:
- Payer, facilitator, and seller wallet addresses
- Polygon RPC URL
- USDC contract address
- Settlement threshold (0.003 USDC)

## Documentation

- `QUICK_START_TS_RUST.md` - Quick reference guide
- `UPTO_TS_INTEGRATION_ANALYSIS.md` - Detailed technical analysis

## Troubleshooting

- **Cap exhausted**: Ensure `maxAmountRequired` in server.ts is sufficient
- **Connection errors**: Verify Rust facilitator is running on port 8090
- **Settlement not triggering**: Check server logs for threshold detection
