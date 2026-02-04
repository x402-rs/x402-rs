import { x402Client, wrapFetchWithPayment } from "@x402/fetch";
import { registerExactEvmScheme } from "@x402/evm/exact/client";
import { privateKeyToAccount } from "viem/accounts";
import { config, getWalletConfig } from "./config.js";
import { spawn } from "child_process";
import { join } from "path";
import { WORKSPACE_ROOT } from "./workspace-root";
import { printLines } from "./process-handle";

export interface ClientOptions {
  facilitatorUrl: string;
  chain: "eip155" | "solana" | "aptos";
  scheme?: string;
  namespace?: string;
  version?: "v1" | "v2";
}

export async function invokeRustClient(
  endpoint: URL,
  privateKeys:
    | {
        eip155: string;
      }
    | { solana: string },
) {
  const binaryPath = new URL(
    "target/debug/x402-reqwest-example",
    WORKSPACE_ROOT,
  ).pathname;
  let env: any = {
    ...process.env,
    ENDPOINT: endpoint.href,
  };
  if ("eip155" in privateKeys) {
    env = {
      ...env,
      EVM_PRIVATE_KEY: privateKeys["eip155"],
    };
  }
  const childProcess = spawn(binaryPath, {
    cwd: WORKSPACE_ROOT.pathname,
    stdio: ["ignore", "pipe", "pipe"],
    env,
  });
  const prefix = `[rs-client]`;
  let stdout = Uint8Array.from([]);
  childProcess.stdout.on("data", (data: Uint8Array) => {
    stdout = new Uint8Array([...stdout, ...data]);
    printLines(process.stdout, prefix, data);
  });
  childProcess.stderr.on("data", (data: Uint8Array) => {
    printLines(process.stderr, prefix, data);
  });
  const exitP = Promise.withResolvers<void>();
  const onError = (err: Error) => {
    childProcess.off("exit", onExit);
    exitP.reject(err);
  };
  const onExit = () => {
    childProcess.off("error", onError);
    exitP.resolve();
  };
  childProcess.on("error", onError);
  childProcess.on("exit", onExit);
  await exitP.promise;
  return new TextDecoder().decode(stdout);
}

// export interface X402Client {
//   fetch(url: string, init?: RequestInit): Promise<Response>;
// }
//
// export function createX402Client(options: ClientOptions): X402Client {
//   const wallets = getWalletConfig(options.chain);
//
//   // Create the x402 client
//   const client = new x402Client();
//
//   // Register EVM scheme for eip155
//   if (options.chain === 'eip155') {
//     const signer = privateKeyToAccount(wallets.payer as `0x${string}`);
//     registerExactEvmScheme(client, { signer });
//   }
//
//   // Wrap fetch with payment handling
//   const fetchWithPayment = wrapFetchWithPayment(fetch, client);
//
//   return {
//     async fetch(url: string, init?: RequestInit): Promise<Response> {
//       return fetchWithPayment(url, init);
//     },
//   };
// }
//
// export interface RustClientOptions {
//   port?: number;
//   facilitatorUrl: string;
// }
//
// export async function startRustClient(options: RustClientOptions): Promise<{ stop: () => Promise<void> }> {
//   const port = options.port ?? config.server.port + 1; // Use different port than server
//   const clientUrl = `http://localhost:${port}`;
//
//   // Check if client is already running
//   try {
//     const response = await fetch(`${clientUrl}/health`, { method: 'GET' });
//     if (response.ok) {
//       console.log(`Rust Client already running at ${clientUrl}`);
//       return {
//         stop: async () => {},
//       };
//     }
//   } catch {
//     // Client not running, need to start it
//   }
//
//   console.log(`Starting Rust test client at ${clientUrl}...`);
//
//   // Start Rust test client via cargo run using x402-reqwest-example
//   const rustClientProcess = spawn('cargo', [
//     'run',
//     '--manifest-path', join(WORKSPACE_ROOT, 'examples/x402-reqwest-example/Cargo.toml'),
//     '--', '--facilitator-url', options.facilitatorUrl,
//   ], {
//     cwd: WORKSPACE_ROOT,
//     stdio: ['ignore', 'pipe', 'pipe'],
//     env: {
//       ...process.env,
//       FACILITATOR_URL: options.facilitatorUrl,
//       CLIENT_PORT: port.toString(),
//     },
//   });
//
//   rustClientProcess.stdout?.on('data', (data) => {
//     process.stdout.write(`[rust-client] ${data}`);
//   });
//
//   rustClientProcess.stderr?.on('data', (data) => {
//     process.stderr.write(`[rust-client] ${data}`);
//   });
//
//   rustClientProcess.on('error', (err) => {
//     console.error(`Rust client error: ${err}`);
//   });
//
//   console.log(`Rust test client started at ${clientUrl}`);
//
//   return {
//     stop: async () => {
//       rustClientProcess.kill('SIGTERM');
//     },
//   };
// }
//
// export async function makePaymentRequest(
//   serverUrl: string,
//   resourcePath: string = '/static-price-v2',
//   options?: {
//     facilitatorUrl?: string;
//     amount?: string;
//     chain?: 'eip155' | 'solana' | 'aptos';
//   }
// ): Promise<Response> {
//   const chain = options?.chain || 'eip155';
//   const wallets = getWalletConfig(chain);
//
//   if (!options?.facilitatorUrl) {
//     throw new Error('Facilitator URL is required');
//   }
//
//   const client = createX402Client({
//     facilitatorUrl: options.facilitatorUrl,
//     chain,
//   });
//
//   return client.fetch(`${serverUrl}${resourcePath}`, {
//     method: 'GET',
//     headers: {
//       'X-Payment-Accepted': '2',
//       'X-Payment-Scheme': 'exact',
//       'X-Payment-Namespace': chain,
//       'X-Payment-Payee': wallets.payee,
//       'X-Payment-Amount': options?.amount || '1',
//     },
//   });
// }
