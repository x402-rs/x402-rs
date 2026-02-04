import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { getWalletConfig } from '../utils/config.js';
import { RSFacilitatorHandle } from "../utils/facilitator";
import { RSServerHandle } from "../utils/server";
import { makeFetch } from "../utils/client";

describe('v2-eip155-exact-ts-rs-rs: x402 v2, eip155, exact, TS Client + Rust Server + Rust Facilitator', () => {
  let facilitator: RSFacilitatorHandle;
  let server: RSServerHandle;

  beforeAll(async () => {
    facilitator = await RSFacilitatorHandle.spawn()
    server = await RSServerHandle.spawn(facilitator.url)
  }, 120000); // 2 minute timeout for starting services

  afterAll(async () => {
    await server.stop();
    await facilitator.stop();
  });

  it('should have facilitator running', async () => {
    const response = await fetch(new URL('./health', facilitator.url));
    expect(response.ok).toBe(true);
  });

  it('should return 402 Payment Required when no payment header on protected endpoint', async () => {
    const response = await fetch(new URL('/static-price-v2', server.url));
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
    const fetchFn = makeFetch('eip155')
    const endpoint = new URL('./static-price-v2', server.url);
    const response = await fetchFn(endpoint);

    // Should succeed with 200 OK
    expect(response.status).toBe(200);

    // Verify the returned content
    const text = await response.text();
    expect(text).toBe("VIP content from /static-price-v2");
  });
});
