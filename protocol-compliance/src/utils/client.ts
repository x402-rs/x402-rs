import { config } from './config.js';

export interface ClientOptions {
  facilitatorUrl: string;
  chain: 'eip155' | 'solana' | 'aptos';
  scheme?: string;
  namespace?: string;
  version?: 'v1' | 'v2';
}

export function createX402Client(_options: ClientOptions) {
  return {
    facilitatorUrl: '',
    chain: 'eip155' as const,
    scheme: 'exact',
    namespace: 'eip155',
    version: 'v2' as const,
    payerPrivateKey: '',
    async fetch(_url: string, _init?: RequestInit): Promise<Response> {
      return new Response('Not implemented', { status: 500 });
    },
  };
}

export async function makePaymentRequest(
  _serverUrl: string,
  _resourcePath: string = '/protected/resource',
  _options?: {
    amount?: string;
    token?: string;
    extra?: Record<string, string>;
  }
): Promise<Response> {
  return new Response('Not implemented', { status: 500 });
}

export function getWalletConfig(chain: 'eip155' | 'solana' | 'aptos') {
  return {
    payer: config.wallets.payer[chain],
    payee: config.wallets.payee[chain],
  };
}
