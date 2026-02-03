import type { ServerHandle } from './facilitator.js';
import { config } from './config.js';
import { waitForUrl } from './waitFor.js';

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
