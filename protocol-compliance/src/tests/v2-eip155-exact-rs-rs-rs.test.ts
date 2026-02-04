import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { RustFacilitatorHandle } from '../utils/facilitator.js';
import { startRustServer } from '../utils/server.js';
import { startRustClient } from '../utils/client.js';
import { config, getWalletConfig } from '../utils/config.js';

describe('v2-eip155-exact-rs-rs-rs: x402 v2, eip155, exact, Rust Client + Rust Server + Rust Facilitator', () => {
  let facilitator: RustFacilitatorHandle;
  // let server: ServerHandle;
  // let client: { stop: () => Promise<void> };

  beforeAll(async () => {
    // Start the local facilitator
    facilitator = await RustFacilitatorHandle.spawn();

    // Start the Rust test server (x402-axum-example)
    // server = await startRustServer({
    //   facilitatorUrl: facilitator.url,
    // });

    // // Start the Rust test client (x402-reqwest-example)
    // client = await startRustClient({
    //   facilitatorUrl: facilitator.url,
    // });
  }, 120000); // 2 minute timeout for starting services

  afterAll(async () => {
    // await client.stop();
    // await server.stop();
    await facilitator.stop();
  });

  it('foo', async () => {
    console.log('foo');
  })

  // it('should have facilitator running', async () => {
  //   const response = await fetch(`${facilitator.url}/health`);
  //   expect(response.ok).toBe(true);
  // });
  //
  // it('should have server running', async () => {
  //   // x402-axum-example listens on port 3000
  //   const response = await fetch(`${server.url}/static-price-v2`);
  //   // Should either get 402 (payment required) or 200 (free endpoint)
  //   expect([200, 402]).toContain(response.status);
  // });
  //
  // it('should return 402 Payment Required when no payment header on protected endpoint', async () => {
  //   const response = await fetch(`${server.url}/static-price-v2`);
  //   // Without payment, should get 402
  //   expect(response.status).toBe(402);
  // });
  //
  // it('should return 200 OK and VIP content when payment is provided via Rust client', async () => {
  //   const wallets = getWalletConfig('eip155');
  //
  //   // Skip if no EVM private key is configured
  //   if (!wallets.payer || wallets.payer.length === 0) {
  //     console.log('Skipping Rust client test - no EVM private key configured');
  //     return;
  //   }
  //
  //   // Make a request using the Rust reqwest client
  //   const response = await fetch(`${server.url}/static-price-v2`, {
  //     method: 'GET',
  //     headers: {
  //       'X-Payment-Accepted': '2',
  //       'X-Payment-Scheme': 'exact',
  //       'X-Payment-Namespace': 'eip155',
  //       'X-Payment-Payee': wallets.payee,
  //       'X-Payment-Amount': '1',
  //     },
  //   });
  //
  //   // Should succeed with 200 OK
  //   expect(response.status).toBe(200);
  //
  //   // Verify the returned content
  //   const text = await response.text();
  //   expect(text).toBe('This is a VIP content!');
  // });
});
