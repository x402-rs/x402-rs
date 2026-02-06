import { Hono } from "hono";
import { serve, ServerType } from "@hono/node-server";
import { paymentMiddleware, x402ResourceServer } from "@x402/hono";
import { ExactEvmScheme } from "@x402/evm/exact/server";
import { HTTPFacilitatorClient } from "@x402/core/server";
import { WORKSPACE_ROOT } from "./workspace-root";
import { ProcessHandle } from "./process-handle";
import { waitForUrl } from "./waitFor";
import { ExactSvmScheme } from "@x402/svm/exact/server";
import getPort from "get-port";

export class RSServerHandle {
  readonly url: URL;
  readonly process: ProcessHandle;

  static async spawn(facilitatorUrl: URL, port?: number) {
    port = port ?? (await getPort());
    const serverUrl = new URL(`http://localhost:${port}/`);

    const serverBinary = new URL(
      "./target/debug/x402-axum-example",
      WORKSPACE_ROOT,
    ).pathname;

    console.log(`Starting Rust server ${serverBinary} at ${serverUrl}...`);

    const serverProcess = ProcessHandle.spawn("rs-server", serverBinary, [], {
      cwd: WORKSPACE_ROOT.pathname,
      stdio: ["ignore", "pipe", "pipe"],
      env: {
        ...process.env,
        FACILITATOR_URL: facilitatorUrl.href,
        PORT: port.toString(),
      },
    });

    const ready = await Promise.race([
      waitForUrl(serverUrl, { timeoutMs: 60000 }),
      serverProcess.waitExit(),
    ]);
    if (!ready) {
      throw new Error(`Rust server failed to start within 60 seconds`);
    }

    console.log(`Rust server started at ${serverUrl}`);
    return new RSServerHandle(serverUrl, serverProcess);
  }

  constructor(url: URL, process: ProcessHandle) {
    this.url = url;
    this.process = process;
  }

  async stop() {
    await this.process.stop();
  }
}

export class TSServerHandle {
  readonly url: URL;
  readonly server: ServerType;

  static async spawn(facilitatorUrl: URL, port?: number) {
    port = port ?? (await getPort());
    const serverUrl = new URL(`http://localhost:${port}/`);
    console.log(`Starting TS test server at ${serverUrl}...`);

    const normalizedFacilitatorUrl = facilitatorUrl.href.replace(/(\/)+$/, "");
    console.log(`Using facilitator at ${normalizedFacilitatorUrl}`);
    const facilitatorClient = new HTTPFacilitatorClient({
      url: normalizedFacilitatorUrl,
    });
    const resourceServer = new x402ResourceServer(facilitatorClient)
      .register("eip155:84532", new ExactEvmScheme())
      .register(
        "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
        new ExactSvmScheme(),
      );

    const app = new Hono();
    // Apply the payment middleware with configuration
    app.use(
      paymentMiddleware(
        {
          "GET /static-price-v2": {
            accepts: [
              {
                scheme: "exact",
                price: "$0.001",
                network: "eip155:84532",
                payTo: "0xBAc675C310721717Cd4A37F6cbeA1F081b1C2a07",
              },
              {
                scheme: "exact",
                price: "$0.001",
                network: "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1",
                payTo: "EGBQqKn968sVv5cQh5Cr72pSTHfxsuzq7o7asqYB5uEV",
              },
            ],
            description: "Access to premium content",
          },
        },
        resourceServer,
      ),
    );

    // Health check endpoint
    app.get("/health", (c) => c.json({ status: "ok" }));

    // Protected route that returns VIP content
    app.get("/static-price-v2", async (c) => {
      return c.text("VIP content from /static-price-v2");
    });

    // Start the server
    const server = await new Promise<ServerType>((resolve, reject) => {
      const onError = (err: unknown) => {
        console.error("Server error:", err);
        reject(err);
      };
      const server = serve({ fetch: app.fetch, port }, () => {
        server.off("error", onError);
        resolve(server);
      });
      server.on("error", onError);
    });

    return new TSServerHandle(serverUrl, server);
  }

  constructor(serverUrl: URL, server: ServerType) {
    this.url = serverUrl;
    this.server = server;
  }

  async stop(): Promise<void> {
    const closeP = Promise.withResolvers<void>();
    this.server.close((err) => {
      err ? closeP.reject(err) : closeP.resolve();
    });
    return closeP.promise;
  }
}
