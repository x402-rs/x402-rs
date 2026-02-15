import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { RSFacilitatorHandle } from "../utils/facilitator.js";
import { ROUTES, TSServerHandle } from "../utils/server.js";
import {
  EIP155_ACCOUNT,
  getAllowance,
  getBalance,
  makeFetch,
  setAllowance,
} from "../utils/client.js";
import { PERMIT2_ADDRESS } from "../utils/erc-abi";

const PATH = "/static-price-v2-permit2";

describe("v2-eip155-exact-ts-ts-rs: permit2: x402 v2, eip155, exact + permit2, TS Client + TS Server + Rust Facilitator", () => {
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
    const response = await fetch(new URL(`${PATH}`, server.url));
    // Should either get 402 (payment required) or 200 (free endpoint)
    expect([200, 402]).toContain(response.status);
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

  it(
    "should return 200 OK and VIP content when payment is provided via TS client",
    { timeout: 10000 },
    async () => {
      const params = ROUTES[PATH];
      const tokenAddress = params.accepts[0].price.asset;
      const amount = BigInt(params.accepts[0].price.amount);
      const balanceBefore = await getBalance(
        tokenAddress,
        EIP155_ACCOUNT.address,
      );
      // Set allowance
      const currentAllowance = await getAllowance(
        tokenAddress,
        PERMIT2_ADDRESS,
      );
      if (currentAllowance !== amount) {
        await setAllowance(tokenAddress, PERMIT2_ADDRESS, amount);
      }
      // Make a request using the TypeScript client (simulated payment headers)
      const fetchFn = await makeFetch("eip155");
      const endpoint = new URL(PATH, server.url);
      const response = await fetchFn(endpoint);

      // Should succeed with 200 OK
      expect(response.status).toBe(200);

      // Verify the returned content
      const text = await response.text();
      expect(text).toBe("VIP content from /static-price-v2-permit2");

      const balanceAfter = await getBalance(
        tokenAddress,
        EIP155_ACCOUNT.address,
      );
      const balanceDelta = balanceAfter - balanceBefore;
      expect(balanceDelta).toBe(-amount);
      const allowanceAfter = await getAllowance(tokenAddress, PERMIT2_ADDRESS);
      expect(allowanceAfter).toBe(0n);
    },
  );
});
