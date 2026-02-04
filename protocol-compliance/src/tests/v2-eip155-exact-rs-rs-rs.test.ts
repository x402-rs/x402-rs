import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { RustFacilitatorHandle } from "../utils/facilitator.js";
import { RustServerHandle } from "../utils/server.js";
import { invokeRustClient } from "../utils/client.js";
import { config } from "../utils/config.js";

describe("v2-eip155-exact-rs-rs-rs: x402 v2, eip155, exact, Rust Client + Rust Server + Rust Facilitator", () => {
  let facilitator: RustFacilitatorHandle;
  let server: RustServerHandle;
  beforeAll(async () => {
    facilitator = await RustFacilitatorHandle.spawn();
    server = await RustServerHandle.spawn(facilitator.url);
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
    // x402-axum-example listens on port 3000
    const response = await fetch(new URL("./static-price-v2", server.url));

    // Should either get 402 (payment required) or 200 (free endpoint)
    expect([200, 402]).toContain(response.status);
  });

  it("should return 402 Payment Required when no payment header on protected endpoint", async () => {
    const response = await fetch(new URL("./static-price-v2", server.url));
    // Without payment, should get 402
    expect(response.status).toBe(402);
  });

  it("should return 200 OK and VIP content when payment is provided via Rust client", async () => {
    const privateKey = config.wallets.payer.eip155;
    if (!privateKey) {
      throw new Error("No private key configured for Rust client test");
    }
    const endpoint = new URL("./static-price-v2", server.url);
    const stdout = await invokeRustClient(endpoint, {
      eip155: privateKey,
    });
    expect(stdout).toContain("VIP content from /static-price-v2");
  });
});
