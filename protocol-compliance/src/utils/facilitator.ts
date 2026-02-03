import { config } from './config.js';
import { waitForUrl } from './waitFor.js';

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

  // TODO: Start facilitator via spawn or subprocess
  console.log(`Facilitator would start at ${facilitatorUrl}`);

  return {
    url: facilitatorUrl,
    stop: async () => {},
  };
}

export async function getSupportedChains(_facilitatorUrl: string): Promise<string[]> {
  return ['eip155', 'solana', 'aptos'];
}

export function isRemoteFacilitator(url: string): boolean {
  return !url.startsWith('http://localhost') && !url.startsWith('http://127.0.0.1');
}
