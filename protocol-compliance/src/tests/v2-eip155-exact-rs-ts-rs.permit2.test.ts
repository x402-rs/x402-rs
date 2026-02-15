import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { RSFacilitatorHandle } from "../utils/facilitator.js";
import { ROUTES, TSServerHandle } from "../utils/server.js";
import { config } from "../utils/config.js";
import {
  EIP155_ACCOUNT,
  getAllowance,
  getBalance,
  invokeRustClient,
  setAllowance,
} from "../utils/client.js";
import { PERMIT2_ADDRESS } from "../utils/erc-abi";

const PATH = "/static-price-v2-permit2";

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
    const response = await fetch(new URL(`${PATH}`, server.url));
    // Without payment, should get 402
    expect(response.status).toBe(402);
  });

  // it("should return 200 OK and VIP content when payment is provided via Rust client", async () => {
  //   const privateKey = config.baseSepolia.buyerPrivateKey;
  //   const endpoint = new URL(`${PATH}`, server.url);
  //   const stdout = await invokeRustClient(endpoint, {
  //     eip155: privateKey,
  //   });
  //   expect(stdout).toContain("VIP content from /static-price-v2");
  // });

  it("should return 412 on zero allowance", { timeout: 10000 }, async () => {
    const privateKey = config.baseSepolia.buyerPrivateKey;
    const endpoint = new URL(`${PATH}`, server.url);
    const params = ROUTES[PATH];
    const tokenAddress = params.accepts[0].price.asset;
    await setAllowance(tokenAddress, PERMIT2_ADDRESS, 0n);

    const stdout = await invokeRustClient(endpoint, {
      eip155: privateKey,
    });

    // Should succeed with 412
    expect(stdout).toContain("Status: 412");
  });

  it(
    "should return 200 OK and VIP content when payment is provided via TS client",
    { timeout: 10000 },
    async () => {
      const privateKey = config.baseSepolia.buyerPrivateKey;
      const endpoint = new URL(`${PATH}`, server.url);
      const params = ROUTES[PATH];
      const tokenAddress = params.accepts[0].price.asset;
      const amount = BigInt(params.accepts[0].price.amount);
      const balanceBefore = await getBalance(
        tokenAddress,
        EIP155_ACCOUNT.address,
      );
      // Set allowance
      await setAllowance(tokenAddress, PERMIT2_ADDRESS, amount);

      const stdout = await invokeRustClient(endpoint, {
        eip155: privateKey,
      });
      expect(stdout).toContain("VIP content from /static-price-v2-permit2");

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
