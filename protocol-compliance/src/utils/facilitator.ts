import { config } from './config.js';
import { waitForUrl } from './waitFor.js';
import { spawn } from 'child_process';
import { join } from 'path';

// Workspace root - hardcoded for vitest compatibility
const WORKSPACE_ROOT = '/Users/ukstv/Developer/FareSide/x402-rs';

export interface ServerHandle {
  url: string;
  stop: () => Promise<void>;
}

export async function startLocalFacilitator(
  port: number = config.facilitator.port
): Promise<ServerHandle> {
  const facilitatorUrl = `http://localhost:${port}`;

  // Check if facilitator is already running
  const alreadyRunning = await waitForUrl(facilitatorUrl, { timeoutMs: 2000 });
  if (alreadyRunning) {
    console.log(`Facilitator already running at ${facilitatorUrl}`);
    return {
      url: facilitatorUrl,
      stop: async () => {},
    };
  }

  // Start facilitator via cargo run with absolute path
  console.log(`Starting facilitator at ${facilitatorUrl}...`);
  console.log(`WORKSPACE_ROOT: ${WORKSPACE_ROOT}`);

  const facilitatorProcess = spawn('cargo', [
    'run',
    '--manifest-path', join(WORKSPACE_ROOT, 'facilitator/Cargo.toml'),
    '--', '--config', join(WORKSPACE_ROOT, 'protocol-compliance/test-config.json')
  ], {
    cwd: WORKSPACE_ROOT,
    stdio: ['ignore', 'pipe', 'pipe'],
    env: {
      ...process.env,
      PORT: port.toString(),
      // Ensure root .env is loaded - Rust dotenv looks for .env in cwd or parent dirs
      DOTENV_CONFIG: 'full',
    },
  });

  facilitatorProcess.stdout?.on('data', (data) => {
    process.stdout.write(`[facilitator] ${data}`);
  });

  facilitatorProcess.stderr?.on('data', (data) => {
    process.stderr.write(`[facilitator] ${data}`);
  });

  facilitatorProcess.on('error', (err) => {
    console.error(`Facilitator error: ${err}`);
  });

  // Wait for facilitator to be ready
  const ready = await waitForUrl(facilitatorUrl, { timeoutMs: 60000 });
  if (!ready) {
    throw new Error(`Facilitator failed to start within 60 seconds`);
  }

  console.log(`Facilitator started at ${facilitatorUrl}`);

  return {
    url: facilitatorUrl,
    stop: async () => {
      facilitatorProcess.kill('SIGTERM');
    },
  };
}

export async function getSupportedChains(facilitatorUrl: string): Promise<string[]> {
  try {
    const response = await fetch(`${facilitatorUrl}/chains`);
    if (!response.ok) {
      throw new Error(`Failed to get chains: ${response.statusText}`);
    }
    return response.json();
  } catch {
    // If the chains endpoint doesn't exist, return default chains
    return ['eip155', 'solana', 'aptos'];
  }
}

export function isRemoteFacilitator(url: string): boolean {
  return !url.startsWith('http://localhost') && !url.startsWith('http://127.0.0.1');
}
