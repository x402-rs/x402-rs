import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { RSFacilitatorHandle } from "../utils/facilitator.js";
import { TSServerHandle } from "../utils/server.js";
import { config } from "../utils/config.js";
import { invokeRustClient } from "../utils/client.js";

describe("v2-eip155-exact-rs-ts-rs: x402 v2, eip155, exact, Rust Client + TS Server + Rust Facilitator", () => {
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

  it("should return 402 Payment Required when no payment header on protected endpoint", async () => {
    const response = await fetch(`${server.url}/static-price-v2`);
    // Without payment, should get 402
    expect(response.status).toBe(402);
  });

  it("should return 200 OK and VIP content when payment is provided via Rust client", async () => {
    const privateKey = config.baseSepolia.buyerPrivateKey;
    const endpoint = new URL("./static-price-v2", server.url);
    const stdout = await invokeRustClient(endpoint, {
      eip155: privateKey,
    });
    expect(stdout).toContain("VIP content from /static-price-v2");
  });
});
