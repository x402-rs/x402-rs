import express from 'express';
import { config } from 'dotenv';
import { createExpressPaidRoutes } from '@daydreamsai/facilitator/express';
import { createResourceServer } from '@daydreamsai/facilitator/server';
import { createUptoModule } from '@daydreamsai/facilitator/upto';
import { HTTPFacilitatorClient } from '@x402/core/http';

config();

const PORT = process.env.PORT || 3000;
const FACILITATOR_URL = process.env.FACILITATOR_URL || 'http://localhost:8090';
const SELLER_ADDRESS = process.env.SELLER_ADDRESS!;
const USDC_ADDRESS = process.env.USDC_ADDRESS || '0x3c499c542cEF5E3811e1192ce70d8cC03d5c3359';

const SETTLEMENT_THRESHOLD = 3000n; // 0.003 USDC (3 × 0.001 USDC)

console.log('🚀 Starting Express server with Rust facilitator');
console.log(`📍 Facilitator URL: ${FACILITATOR_URL}`);
console.log(`💰 Price: 0.001 USDC per request`);
console.log(`💵 Settlement threshold: 0.003 USDC (3 payments)\n`);

// Create facilitator client pointing to Rust facilitator
const facilitatorClient = new HTTPFacilitatorClient({
  url: FACILITATOR_URL,
});

// Create resource server (handles payment verification)
const resourceServer = createResourceServer(facilitatorClient, {
  exactEvm: false,  // Disable exact scheme
  uptoEvm: true,    // Enable upto scheme
});

// Create upto module for session tracking
const upto = createUptoModule({
  facilitatorClient,
  autoTrack: true,  // Auto-track upto sessions
});

const app = express();

// Create paid routes with upto middleware
const paidRoutes = createExpressPaidRoutes(app, {
  basePath: '/',
  middleware: {
    resourceServer,
    upto,
    autoSettle: false, // Upto doesn't auto-settle anyway
  },
});

// Protected endpoint with price tag configuration
paidRoutes.get(
  '/protected',
  (req, res) => {
    const sessionId = req.headers['x-upto-session-id'];
    
    // Check threshold and trigger settlement if needed
    const x402 = (req as any).x402;
    if (x402?.result?.type === 'payment-verified' && 
        x402.result.paymentRequirements.scheme === 'upto' &&
        x402.tracking?.success) {
      
      const sessionIdFromTracking = x402.tracking.sessionId;
      const session = upto.store.get(sessionIdFromTracking);
      
      if (session) {
        if (session.pendingSpent >= SETTLEMENT_THRESHOLD) {
          console.log(`\n💰 Threshold reached! pendingSpent: ${session.pendingSpent}, threshold: ${SETTLEMENT_THRESHOLD}`);
          console.log(`🔄 Triggering settlement for session: ${sessionIdFromTracking}`);
          
          // Trigger settlement asynchronously (don't wait for it)
          upto.settleSession(sessionIdFromTracking, 'threshold_reached', false)
            .then(() => {
              const updatedSession = upto.store.get(sessionIdFromTracking);
              if (updatedSession?.lastSettlement?.receipt.success) {
                console.log(`✅ Settlement successful!`);
                console.log(`   Transaction: ${updatedSession.lastSettlement.receipt.transaction}`);
                console.log(`   Amount settled: 0.003 USDC`);
              } else {
                console.log(`⚠️  Settlement may have failed or is pending`);
              }
            })
            .catch((error) => {
              console.error(`❌ Settlement error:`, error);
            });
        } else {
          console.log(`📊 Payment tracked. pendingSpent: ${session.pendingSpent}/${SETTLEMENT_THRESHOLD}`);
        }
      }
    }
    
    res.json({
      message: '🎉 Protected content accessed successfully!',
      scheme: 'upto',
      sessionId: sessionId || 'none',
      note: 'Payment verified and tracked (batched)'
    });
  },
  {
    payment: {
      accepts: {
        scheme: 'upto',
        network: 'eip155:137',
        payTo: SELLER_ADDRESS,
        price: {
          amount: '1000', // 0.001 USDC (6 decimals)
          asset: USDC_ADDRESS,
          extra: {
            name: 'USD Coin',
            version: '2',
            maxAmountRequired: '10000', // 0.01 USDC cap (allows 10 payments, enough for 3)
          },
        },
      },
      description: 'Protected API endpoint',
      mimeType: 'application/json',
    },
  }
);

app.listen(PORT, () => {
  console.log(`\n✅ Server listening on http://localhost:${PORT}`);
  console.log(`📍 Protected endpoint: http://localhost:${PORT}/protected`);
  console.log(`💡 Make 3 API calls to trigger settlement at 0.003 USDC threshold\n`);
});
