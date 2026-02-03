import { x402Client, wrapFetchWithPayment } from '@x402/fetch';
import { registerExactEvmScheme } from '@x402/evm/exact/client';
import { privateKeyToAccount } from 'viem/accounts';
import { config, getWalletConfig } from './config.js';
import { spawn } from 'child_process';
import { join } from 'path';

// Workspace root - hardcoded for vitest compatibility
const WORKSPACE_ROOT = '/Users/ukstv/Developer/FareSide/x402-rs';

export interface ClientOptions {
  facilitatorUrl: string;
  chain: 'eip155' | 'solana' | 'aptos';
  scheme?: string;
  namespace?: string;
  version?: 'v1' | 'v2';
}

export interface X402Client {
  fetch(url: string, init?: RequestInit): Promise<Response>;
}

export function createX402Client(options: ClientOptions): X402Client {
  const wallets = getWalletConfig(options.chain);

  // Create the x402 client
  const client = new x402Client();

  // Register EVM scheme for eip155
  if (options.chain === 'eip155') {
    const signer = privateKeyToAccount(wallets.payer as `0x${string}`);
    registerExactEvmScheme(client, { signer });
  }

  // Wrap fetch with payment handling
  const fetchWithPayment = wrapFetchWithPayment(fetch, client);

  return {
    async fetch(url: string, init?: RequestInit): Promise<Response> {
      return fetchWithPayment(url, init);
    },
  };
}

export interface RustClientOptions {
  port?: number;
  facilitatorUrl: string;
}

export async function startRustClient(options: RustClientOptions): Promise<{ stop: () => Promise<void> }> {
  const port = options.port ?? config.server.port + 1; // Use different port than server
  const clientUrl = `http://localhost:${port}`;

  // Check if client is already running
  try {
    const response = await fetch(`${clientUrl}/health`, { method: 'GET' });
    if (response.ok) {
      console.log(`Rust Client already running at ${clientUrl}`);
      return {
        stop: async () => {},
      };
    }
  } catch {
    // Client not running, need to start it
  }

  console.log(`Starting Rust test client at ${clientUrl}...`);

  // Start Rust test client via cargo run using x402-reqwest-example
  const rustClientProcess = spawn('cargo', [
    'run',
    '--manifest-path', join(WORKSPACE_ROOT, 'examples/x402-reqwest-example/Cargo.toml'),
    '--', '--facilitator-url', options.facilitatorUrl,
  ], {
    cwd: WORKSPACE_ROOT,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: {
      ...process.env,
      FACILITATOR_URL: options.facilitatorUrl,
      CLIENT_PORT: port.toString(),
    },
  });

  rustClientProcess.stdout?.on('data', (data) => {
    process.stdout.write(`[rust-client] ${data}`);
  });

  rustClientProcess.stderr?.on('data', (data) => {
    process.stderr.write(`[rust-client] ${data}`);
  });

  rustClientProcess.on('error', (err) => {
    console.error(`Rust client error: ${err}`);
  });

  console.log(`Rust test client started at ${clientUrl}`);

  return {
    stop: async () => {
      rustClientProcess.kill('SIGTERM');
    },
  };
}

export async function makePaymentRequest(
  serverUrl: string,
  resourcePath: string = '/static-price-v2',
  options?: {
    facilitatorUrl?: string;
    amount?: string;
    chain?: 'eip155' | 'solana' | 'aptos';
  }
): Promise<Response> {
  const chain = options?.chain || 'eip155';
  const wallets = getWalletConfig(chain);

  const client = createX402Client({
    facilitatorUrl: options?.facilitatorUrl || config.facilitator.url,
    chain,
  });

  return client.fetch(`${serverUrl}${resourcePath}`, {
    method: 'GET',
    headers: {
      'X-Payment-Accepted': '2',
      'X-Payment-Scheme': 'exact',
      'X-Payment-Namespace': chain,
      'X-Payment-Payee': wallets.payee,
      'X-Payment-Amount': options?.amount || '1',
    },
  });
}

export function getWalletConfig(chain: 'eip155' | 'solana' | 'aptos') {
  return {
    payer: config.wallets.payer[chain],
    payee: config.wallets.payee[chain],
  };
}
