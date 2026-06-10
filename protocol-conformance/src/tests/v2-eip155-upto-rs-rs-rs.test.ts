import { describe, it, expect, beforeAll, afterAll } from "vitest";
import { RSFacilitatorHandle } from "../utils/facilitator.js";
import { ROUTES, RSServerHandle } from "../utils/server.js";
import {
  EIP155_ACCOUNT,
  getAllowance,
  getBalance,
  invokeRustClientUptoEip155,
  setAllowance,
} from "../utils/client.js";
import { PERMIT2_ADDRESS } from "@x402/evm";
import { config, TEST_CONFIG } from "../utils/config.js";

const PATH = "/eip155-upto";

// The RS server (x402-axum-example) configures the upto price at 130 atomic units.
// We set a generous Permit2 allowance so the RS client can sign for the full amount.
const RS_SERVER_MAX_AMOUNT = 130n;

describe("v2-eip155-upto-rs-rs-rs: x402 v2, eip155, upto, Rust Client + Rust Server + Rust Facilitator", () => {
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
    const response = await fetch(new URL(PATH, server.url));
    // Without payment, should get 402
    expect(response.status).toBe(402);
  });

  it(
    "should return 200 OK and VIP content when payment is provided via Rust upto client",
    TEST_CONFIG,
    async () => {
      const tokenAddress = ROUTES[PATH].accepts[0].price.asset;

      const balanceBefore = await getBalance(
        tokenAddress,
        EIP155_ACCOUNT.address,
      );

      // Ensure the buyer has sufficient Permit2 allowance for the RS server's max amount.
      // The RS server presents a max price of RS_SERVER_MAX_AMOUNT atomic units,
      // so Permit2 must allow at least that from the buyer's wallet.
      await setAllowance(tokenAddress, PERMIT2_ADDRESS, RS_SERVER_MAX_AMOUNT);

      // Invoke the Rust upto client binary against the Rust server
      const endpoint = new URL(PATH, server.url);
      const stdout = await invokeRustClientUptoEip155(
        endpoint,
        config.baseSepolia.buyerPrivateKey,
      );

      expect(stdout).toContain("VIP content from /eip155-upto");

      const balanceAfter = await getBalance(
        tokenAddress,
        EIP155_ACCOUNT.address,
      );
      const balanceDelta = balanceAfter - balanceBefore;
      // Some tokens were transferred to the seller
      expect(balanceDelta).toBeLessThan(0n);
      // The settled amount must not exceed the authorized maximum
      expect(-balanceDelta).toBeLessThanOrEqual(RS_SERVER_MAX_AMOUNT);

      const allowanceAfter = await getAllowance(tokenAddress, PERMIT2_ADDRESS);
      // Remaining Permit2 allowance should be less than the original
      expect(allowanceAfter).toBeLessThan(RS_SERVER_MAX_AMOUNT);
    },
  );
});
