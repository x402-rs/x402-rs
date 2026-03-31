import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { RSFacilitatorHandle } from "../utils/facilitator.js";
import { TSServerHandle } from "../utils/server.js";
import { makeFetch } from "../utils/client.js";

describe("v2-solana-exact-ts-ts-rs: x402 v2, solana, exact, TS Client + TS Server + Rust Facilitator", () => {
  let facilitator: RSFacilitatorHandle;
  let server: TSServerHandle;

  beforeAll(async () => {
    facilitator = await RSFacilitatorHandle.spawn();
    server = await TSServerHandle.spawn(facilitator.url);
  }, 120000); // 2 minute timeout for starting services

  afterAll(async () => {
    await server.stop();
    await facilitator.stop();
  });

  it("should have facilitator running", async () => {
    const response = await fetch(new URL("./health", facilitator.url));
    expect(response.ok).toBe(true);
  });

  it("should have server running", async () => {
    const response = await fetch(new URL("./static-price-v2", server.url));
    // Should either get 402 (payment required) or 200 (free endpoint)
    expect([200, 402]).toContain(response.status);
  });

  it("should return 402 Payment Required when no payment header on protected endpoint", async () => {
    const response = await fetch(`${server.url}/static-price-v2`);
    // Without payment, should get 402
    expect(response.status).toBe(402);
  });

  it("should return 200 OK and VIP content when payment is provided via TS client", async () => {
    // Make a request using the TypeScript client (simulated payment headers)
    const fetchFn = await makeFetch("solana");
    const endpoint = new URL("./static-price-v2", server.url);
    const response = await fetchFn(endpoint);

    // Should succeed with 200 OK
    expect(response.status).toBe(200);

    // Verify the returned content
    const text = await response.text();
    expect(text).toBe("VIP content from /static-price-v2");
  });
});
