import { createMiddleware } from "hono/factory";
import {
  PaywallConfig,
  x402ResourceServer,
  type RouteConfig,
  PaywallProvider,
  RoutesConfig,
} from "@x402/core/server";
import { HTTPRequestContext, x402HTTPResourceServer } from "@x402/core/http";
import { HonoAdapter } from "@x402/hono";
import { PaymentPayload, PaymentRequirements } from "@x402/core/types";

export function paymentRequired(
  route: RouteConfig,
  server: x402ResourceServer,
  paywallConfig?: PaywallConfig,
  paywall?: PaywallProvider,
  syncFacilitatorOnStart?: boolean,
) {
  const httpServer = new x402HTTPResourceServer(server, { "*": route });
  // Register custom paywall provider if provided
  if (paywall) {
    httpServer.registerPaywallProvider(paywall);
  }

  // Store initialization promise (not the result)
  // httpServer.initialize() fetches facilitator support and validates routes
  let initPromise: Promise<void> | null = syncFacilitatorOnStart
    ? httpServer.initialize()
    : null;

  // Dynamically register bazaar extension if routes declare it and not already registered
  // Skip if pre-registered (e.g., in serverless environments where static imports are used)
  let bazaarPromise: Promise<void> | null = null;
  if (
    checkIfBazaarNeeded(httpServer.routes) &&
    !httpServer.server.hasExtension("bazaar")
  ) {
    // @ts-ignore
    bazaarPromise = import("@x402/extensions/bazaar")
      .then(({ bazaarResourceServerExtension }) => {
        httpServer.server.registerExtension(bazaarResourceServerExtension);
      })
      .catch((err) => {
        console.error("Failed to load bazaar extension:", err);
      });
  }

  return createMiddleware<{
    Variables: {
      paymentPayload: PaymentPayload,
      requirements: PaymentRequirements,
      setAmountToSettle: (amount: string | bigint) => void
    }
  }>(async (c, next) => {
    // Create adapter and context
    const adapter = new HonoAdapter(c);
    const context: HTTPRequestContext = {
      adapter,
      path: c.req.path,
      method: c.req.method,
      paymentHeader:
        adapter.getHeader("payment-signature") ||
        adapter.getHeader("x-payment"),
    };

    // Check if route requires payment before initializing facilitator
    if (!httpServer.requiresPayment(context)) {
      return next();
    }

    // Only initialize when processing a protected route
    if (initPromise) {
      await initPromise;
      initPromise = null; // Clear after first await
    }

    // Await bazaar extension loading if needed
    if (bazaarPromise) {
      await bazaarPromise;
      bazaarPromise = null;
    }

    // Process payment requirement check
    const result = await httpServer.processHTTPRequest(context, paywallConfig);

    // Handle the different result types
    switch (result.type) {
      case "no-payment-required":
        // No payment needed, proceed directly to the route handler
        return next();

      case "payment-error":
        // Payment required but not provided or invalid
        const { response } = result;
        Object.entries(response.headers).forEach(([key, value]) => {
          c.header(key, value);
        });
        if (response.isHtml) {
          return c.html(response.body as string, response.status as 402);
        } else {
          return c.json(response.body || {}, response.status as 402);
        }

      case "payment-verified":
        // Payment is valid, need to wrap response for settlement
        const { paymentPayload, paymentRequirements, declaredExtensions } =
          result;

        // Proceed to the next middleware or route handler
        c.set('paymentPayload', paymentPayload);
        c.set('requirements', paymentRequirements);
        const setAmountToSettle = (amount: string | bigint) => {
          paymentRequirements.amount = String(amount)
        }
        c.set("setAmountToSettle", setAmountToSettle);
        await next();

        // Get the current response
        let res = c.res;

        // If the response from the protected route is >= 400, do not settle payment
        if (res.status >= 400) {
          return;
        }

        // Clear the response so we can modify headers
        c.res = undefined;

        try {
          const settleResult = await httpServer.processSettlement(
            paymentPayload,
            paymentRequirements,
            declaredExtensions,
          );

          if (!settleResult.success) {
            // Settlement failed - do not return the protected resource
            res = c.json(
              {
                error: "Settlement failed",
                details: settleResult.errorReason,
              },
              402,
            );
          } else {
            // Settlement succeeded - add headers to response
            Object.entries(settleResult.headers).forEach(([key, value]) => {
              res.headers.set(key, value);
            });
          }
        } catch (error) {
          console.error(error);
          // If settlement fails, return an error response
          res = c.json(
            {
              error: "Settlement failed",
              details: error instanceof Error ? error.message : "Unknown error",
            },
            402,
          );
        }

        // Restore the response (potentially modified with settlement headers)
        c.res = res;
        return;
    }
  });
}

function checkIfBazaarNeeded(routes: RoutesConfig): boolean {
  // Handle single route config
  if ("accepts" in routes) {
    return !!(routes.extensions && "bazaar" in routes.extensions);
  }

  // Handle multiple routes
  return Object.values(routes).some((routeConfig) => {
    return !!(routeConfig.extensions && "bazaar" in routeConfig.extensions);
  });
}
