# Quick Start: TypeScript Client/Middleware + Rust Facilitator

## TL;DR

✅ **Yes, you can use `@daydreamsai/facilitator/client` and `@daydreamsai/facilitator/express` with Rust facilitator as-is!**

The TypeScript libraries already support upto scheme and work with any facilitator that implements the standard x402 protocol.

## Installation

```bash
npm install @daydreamsai/facilitator @x402/core viem
```

## Client Setup (TypeScript)

```typescript
import { createUnifiedClient } from "@daydreamsai/facilitator/client";
import { createWalletClient, createPublicClient, http } from "viem";
import { polygon } from "viem/chains";
import { privateKeyToAccount } from "viem/accounts";

// Setup wallet and public client
const account = privateKeyToAccount(process.env.PAYER_PRIVATE_KEY!);
const walletClient = createWalletClient({
  chain: polygon,
  transport: http(),
  account,
});

const publicClient = createPublicClient({
  chain: polygon,
  transport: http(),
});

// Create unified client with upto scheme
const { fetchWithPayment } = createUnifiedClient({
  evmUpto: {
    signer: account,
    publicClient,
    facilitatorUrl: "http://localhost:8090", // Rust facilitator
    deadlineBufferSec: 60,
  },
});

// Use it!
const response = await fetchWithPayment("http://localhost:3000/api/protected");
const data = await response.json();
```

## Server Setup (Express + TypeScript)

```typescript
import express from "express";
import { createExpressPaymentMiddleware } from "@daydreamsai/facilitator/express";
import { createResourceServer } from "@daydreamsai/facilitator/server";
import { createUptoModule } from "@daydreamsai/facilitator/upto";
import { HTTPFacilitatorClient } from "@x402/core/http";

const app = express();

// Facilitator client pointing to Rust facilitator
const facilitatorClient = new HTTPFacilitatorClient({
  url: process.env.FACILITATOR_URL || "http://localhost:8090",
});

// Resource server (handles payment verification)
const resourceServer = createResourceServer(facilitatorClient, {
  exactEvm: false,  // Disable if only using upto
  uptoEvm: true,    // Enable upto scheme
});

// Upto module (handles session tracking)
const upto = createUptoModule({
  facilitatorClient,
  autoTrack: true,  // Auto-track upto sessions
});

// Apply middleware
app.use(
  "/api",
  createExpressPaymentMiddleware({
    resourceServer,
    upto,
    autoSettle: false, // Upto doesn't auto-settle anyway
  })
);

// Protected route
app.get("/api/protected", (req, res) => {
  const sessionId = req.headers["x-upto-session-id"];
  res.json({
    message: "Protected content",
    sessionId,
  });
});

app.listen(3000);
```

## How It Works

1. **Client** (`createUnifiedClient`):
   - Creates EIP-2612 permits
   - Caches permits for reuse (batching)
   - Handles permit invalidation on errors
   - Sends `Payment-Signature` header

2. **Middleware** (`createExpressPaymentMiddleware`):
   - Extracts payment header
   - Calls Rust facilitator `/verify` endpoint
   - Tracks upto sessions (via upto module)
   - **Does NOT settle** (correct batching behavior)
   - Returns protected content

3. **Rust Facilitator**:
   - Verifies permit signatures
   - Handles nonce/allowance checks
   - Settles payments on-chain (when triggered)

4. **Upto Module**:
   - Tracks sessions server-side
   - Provides settlement triggers
   - Can use sweeper for automatic settlement

## Settlement

Settlement happens separately (not during request):

```typescript
// Manual settlement
await upto.settleSession(sessionId, "manual");

// Or use sweeper for automatic settlement
const sweeper = upto.createSweeper({
  intervalMs: 60_000,  // Every minute
  idleSettleMs: 300_000, // After 5 minutes idle
});
```

## Key Points

✅ **No changes needed** - TypeScript libraries work with Rust facilitator as-is
✅ **Proper batching** - Middleware correctly skips settlement for upto
✅ **Session tracking** - Upto module handles session management
✅ **Production ready** - All components are battle-tested

## See Also

- Full analysis: `UPTO_TS_INTEGRATION_ANALYSIS.md`
- Working example: `examples/upto-ts-integration/`
- Upto scheme docs: `upto_scheme.md`
