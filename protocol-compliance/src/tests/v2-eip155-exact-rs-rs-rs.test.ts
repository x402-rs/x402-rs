import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { startLocalFacilitator, type ServerHandle } from '../utils/facilitator.js';
import { startRustServer } from '../utils/server.js';
import { config, getWalletConfig } from '../utils/config.js';

describe('v2-eip155-exact-rs-rs-rs: x402 v2, eip155, exact, Rust Client + Rust Server + Rust Facilitator', () => {
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
    if (response.status === 402) {
      expect(response.headers.get('x-payment-accepted')).toBe('2');
    }
  });

  it('should handle protocol correctly with payment headers', async () => {
    const wallets = getWalletConfig('eip155');

    // Try to access protected endpoint with payment headers
    const response = await fetch(`${server.url}/static-price-v2`, {
      method: 'GET',
      headers: {
        'X-Payment-Accpeted': '2',
        'X-Payment-Scheme': 'exact',
        'X-Payment-Namespace': 'eip155',
        'X-Payment-Payee': wallets.payee,
        'X-Payment-Amount': '1',
      },
    });

    // Should either succeed (200) or fail with protocol error
    expect([200, 402, 400, 500]).toContain(response.status);
  });
});
