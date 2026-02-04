import { Hono } from 'hono';
import { serve } from '@hono/node-server';
import { paymentMiddleware, x402ResourceServer } from '@x402/hono';
import { ExactEvmScheme } from '@x402/evm/exact/server';
import { HTTPFacilitatorClient } from '@x402/core/server';
import { config } from './config.js';
import { spawn } from 'child_process';
import { join } from 'path';
import {WORKSPACE_ROOT} from "./workspace-root";

// Workspace root - hardcoded for vitest compatibility
export interface ServerHandle {
  url: string;
  stop: () => Promise<void>;
}

export interface RustServerOptions {
  facilitatorUrl: string;
  port?: number;
}

export async function startRustServer(options: RustServerOptions): Promise<ServerHandle> {
  const port = options.port ?? config.server.port;
  const serverUrl = `http://localhost:${port}`;

  const serverBinary = new URL('./target/debug/x402-axum-example', WORKSPACE_ROOT).pathname;

  console.log(`Starting Rust server ${serverBinary} at ${serverUrl}...`);

  // Start Rust test server via cargo run using x402-axum-example
  const rustServerProcess = spawn(serverBinary, [], {
    cwd: WORKSPACE_ROOT.pathname,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: {
      ...process.env,
      FACILITATOR_URL: options.facilitatorUrl,
      PORT: port.toString(),
    },
  });

  rustServerProcess.stdout?.on('data', (data) => {
    process.stdout.write(`[rust-server] ${data}`);
  });

  rustServerProcess.stderr?.on('data', (data) => {
    process.stderr.write(`[rust-server] ${data}`);
  });

  rustServerProcess.on('error', (err) => {
    console.error(`Rust server error: ${err}`);
  });

  console.log(`Rust test server started at ${serverUrl}`);

  return {
    url: serverUrl,
    stop: async () => {
      rustServerProcess.kill('SIGTERM');
    },
  };
}

export interface TSServerOptions {
  facilitatorUrl: string;
  port?: number;
  chain?: 'eip155' | 'solana' | 'aptos';
  payeeAddress?: string;
  price?: string;
}

export async function startTSServer(options: TSServerOptions): Promise<ServerHandle> {
  const port = options.port ?? config.server.port;
  const chain = options.chain ?? 'eip155';
  const payeeAddress = options.payeeAddress ?? config.wallets.payee[chain];
  const price = options.price ?? '$0.001';
  const serverUrl = `http://localhost:${port}`;

  // Check if server is already running
  try {
    const response = await fetch(`${serverUrl}/health`, { method: 'GET' });
    if (response.ok) {
      console.log(`TS Server already running at ${serverUrl}`);
      return {
        url: serverUrl,
        stop: async () => {},
      };
    }
  } catch {
    // Server not running, need to start it
  }

  console.log(`Starting TS test server at ${serverUrl}...`);

  // Build the namespace string for the network
  let namespace: string;
  let scheme: string;

  if (chain === 'eip155') {
    namespace = 'eip155:84532'; // Base Sepolia
    scheme = 'exact';
  } else if (chain === 'solana') {
    namespace = 'solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1'; // Solana Devnet
    scheme = 'exact';
  } else {
    namespace = 'aptos'; // Placeholder for Aptos
    scheme = 'exact';
  }

  const app = new Hono();
  const facilitatorClient = new HTTPFacilitatorClient({ url: options.facilitatorUrl });
  const resourceServer = new x402ResourceServer(facilitatorClient);

  // Register the appropriate scheme based on chain
  if (chain === 'eip155') {
    resourceServer.register(namespace, new ExactEvmScheme());
  } else if (chain === 'solana') {
    // For Solana, we'd need ExactSvmScheme
    // resourceServer.register(namespace, new ExactSvmScheme());
    throw new Error('Solana TS server not yet implemented');
  } else {
    throw new Error('Aptos TS server not yet implemented');
  }

  // Apply the payment middleware with configuration
  app.use(
    paymentMiddleware(
      {
        'GET /static-price-v2': {
          accepts: [
            {
              scheme,
              price,
              network: namespace,
              payTo: payeeAddress,
            },
          ],
          description: 'Access to premium content',
        },
      },
      resourceServer,
    ),
  );

  // Health check endpoint
  app.get('/health', (c) => c.json({ status: 'ok' }));

  // Protected route that returns VIP content
  app.get('/static-price-v2', async (c) => {
    return c.text('This is a VIP content!');
  });

  // Start the server
  const server = serve({ fetch: app.fetch, port });

  console.log(`TS test server started at ${serverUrl}`);

  return {
    url: serverUrl,
    stop: async () => {
      server.close();
    },
  };
}
