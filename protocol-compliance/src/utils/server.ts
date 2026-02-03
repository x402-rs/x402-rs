import type { ServerHandle } from './facilitator.js';
import { config } from './config.js';
import { waitForUrl } from './waitFor.js';
import { spawn } from 'child_process';
import { join } from 'path';

// Workspace root - hardcoded for vitest compatibility
const WORKSPACE_ROOT = '/Users/ukstv/Developer/FareSide/x402-rs';

export interface RustServerOptions {
  port?: number;
  facilitatorUrl: string;
}

export async function startRustServer(options: RustServerOptions): Promise<ServerHandle> {
  const port = options.port ?? config.server.port;
  const serverUrl = `http://localhost:${port}`;

  // Check if server is already running
  const alreadyRunning = await waitForUrl(serverUrl, { timeoutMs: 2000 });
  if (alreadyRunning) {
    console.log(`Rust Server already running at ${serverUrl}`);
    return {
      url: serverUrl,
      stop: async () => {},
    };
  }

  // Start Rust test server via cargo run using x402-axum-example
  console.log(`Starting Rust test server at ${serverUrl}...`);

  const rustServerProcess = spawn('cargo', [
    'run',
    '--manifest-path', join(WORKSPACE_ROOT, 'examples/x402-axum-example/Cargo.toml'),
    '--', '--facilitator-url', options.facilitatorUrl,
  ], {
    cwd: WORKSPACE_ROOT,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: {
      ...process.env,
      FACILITATOR_URL: options.facilitatorUrl,
      SERVER_PORT: port.toString(),
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

  // Wait for server to be ready
  const ready = await waitForUrl(serverUrl, { timeoutMs: 60000 });
  if (!ready) {
    throw new Error(`Rust server failed to start within 60 seconds`);
  }

  console.log(`Rust test server started at ${serverUrl}`);

  return {
    url: serverUrl,
    stop: async () => {
      rustServerProcess.kill('SIGTERM');
    },
  };
}

export interface TSServerOptions {
  port?: number;
  facilitatorUrl: string;
  scheme?: string;
  namespace?: string;
  version?: 'v1' | 'v2';
}

export async function startTSServer(_options: TSServerOptions): Promise<ServerHandle> {
  const port = _options.port ?? config.server.port;
  const serverUrl = `http://localhost:${port}`;

  // Check if server is already running
  const alreadyRunning = await waitForUrl(serverUrl, { timeoutMs: 2000 });
  if (alreadyRunning) {
    console.log(`TS Server already running at ${serverUrl}`);
    return {
      url: serverUrl,
      stop: async () => {},
    };
  }

  // TODO: Implement TS server using @x402/hono
  console.log(`TS Server would start at ${serverUrl}`);

  return {
    url: serverUrl,
    stop: async () => {},
  };
}

export function createPaymentHeaders(
  _x402Version: string,
  _scheme: string,
  _namespace: string,
  _payee: string,
  _amount: string,
  _token?: string,
  _extra?: Record<string, string>
): Record<string, string> {
  return {};
}
