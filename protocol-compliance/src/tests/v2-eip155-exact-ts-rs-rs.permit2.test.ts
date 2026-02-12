import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { RSFacilitatorHandle } from "../utils/facilitator.js";
import { ROUTES, RSServerHandle } from "../utils/server.js";
import { makeFetch, setAllowance } from "../utils/client.js";
import { PERMIT2_ADDRESS } from "../utils/erc-abi";

const PATH = "/static-price-v2-permit2";

describe("v2-eip155-exact-ts-rs-rs: x402 v2, eip155, exact, TS Client + Rust Server + Rust Facilitator", () => {
  let facilitator: RSFacilitatorHandle;
  let server: RSServerHandle;

  beforeAll(async () => {
    facilitator = await RSFacilitatorHandle.spawn();
    server = await RSServerHandle.spawn(facilitator.url);
  }, 120000); // 2 minute timeout for starting services

  afterAll(async () => {
    await server.stop();
    await facilitator.stop();
  });

  it("should have facilitator running", async () => {
    const response = await fetch(new URL("./health", facilitator.url));
    expect(response.ok).toBe(true);
  });

  it("should return 402 Payment Required when no payment header on protected endpoint", async () => {
    const response = await fetch(new URL(`${PATH}`, server.url));
    // Without payment, should get 402
    expect(response.status).toBe(402);
  });

  it("should return 412 on zero allowance", { timeout: 10000 }, async () => {
    const params = ROUTES[PATH];
    const tokenAddress = params.accepts[0].price.asset;
    await setAllowance(tokenAddress, PERMIT2_ADDRESS, 0n);

    // Make a request using the TypeScript client (simulated payment headers)
    const fetchFn = await makeFetch("eip155");
    const endpoint = new URL(PATH, server.url);
    const response = await fetchFn(endpoint);

    // Should succeed with 412
    expect(response.status).toBe(412);
  });

  // it("should return 200 OK and VIP content when payment is provided via TS client", async () => {
  //   // Make a request using the TypeScript client (simulated payment headers)
  //   const fetchFn = await makeFetch("eip155");
  //   const endpoint = new URL("./static-price-v2", server.url);
  //   const response = await fetchFn(endpoint);
  //
  //   // Should succeed with 200 OK
  //   expect(response.status).toBe(200);
  //
  //   // Verify the returned content
  //   const text = await response.text();
  //   expect(text).toBe("VIP content from /static-price-v2");
  // });
});
