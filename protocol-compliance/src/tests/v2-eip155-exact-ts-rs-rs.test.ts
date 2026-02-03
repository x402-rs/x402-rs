import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { startLocalFacilitator, type ServerHandle } from '../utils/facilitator.js';
import { startRustServer } from '../utils/server.js';
import { makePaymentRequest } from '../utils/client.js';
import { getWalletConfig } from '../utils/config.js';

describe('v2-eip155-exact-ts-rs-rs: x402 v2, eip155, exact, TS Client + Rust Server + Rust Facilitator', () => {
  let facilitator: ServerHandle;
  let server: ServerHandle;

  beforeAll(async () => {
    // Start the local facilitator
    facilitator = await startLocalFacilitator();

    // Start the Rust test server (x402-axum-example)
    server = await startRustServer({
      facilitatorUrl: facilitator.url,
    });
  }, 120000); // 2 minute timeout for starting services

  afterAll(async () => {
    await server.stop();
    await facilitator.stop();
  });

  it('should have facilitator running', async () => {
    const response = await fetch(`${facilitator.url}/health`);
    expect(response.ok).toBe(true);
  });

  it('should have server running', async () => {
    // x402-axum-example listens on port 3000
    const response = await fetch(`${server.url}/static-price-v2`);
    // Should either get 402 (payment required) or 200 (free endpoint)
    expect([200, 402]).toContain(response.status);
  });

  it('should return 402 Payment Required when no payment header on protected endpoint', async () => {
    const response = await fetch(`${server.url}/static-price-v2`);
    // Without payment, should get 402
    expect(response.status).toBe(402);
  });

  it('should return 200 OK and VIP content when payment is provided via TS client', async () => {
    // Skip if no EVM private key is configured
    const wallets = getWalletConfig('eip155');
    if (!wallets.payer || wallets.payer.length === 0) {
      console.log('Skipping TS client test - no EVM private key configured');
      return;
    }

    // Make a request using the TypeScript client (simulated payment headers)
    // The TS client uses @x402/fetch which constructs proper payment headers
    const response = await makePaymentRequest(server.url, '/static-price-v2', {
      facilitatorUrl: facilitator.url,
    });

    // Should succeed with 200 OK
    expect(response.status).toBe(200);

    // Verify the returned content
    const text = await response.text();
    expect(text).toBe('This is a VIP content!');
  });
});
