import dotenv from 'dotenv';
import getPort from "get-port";

dotenv.config();

export const config = {
  facilitator: {
    port: await getPort(),
  },
  server: {
    port: await getPort(),
  },
  chains: {
    eip155: {
      rpcUrl: process.env.EIP155_RPC_URL || 'https://eth-sepolia.g.alchemy.com/v2/demo',
      network: 'sepolia',
    },
    solana: {
      rpcUrl: process.env.SOLANA_RPC_URL || 'https://api.devnet.solana.com',
      network: 'devnet',
    },
  },
  wallets: {
    payer: {
      eip155: process.env.EVM_PAYER_PRIVATE_KEY || '',
      solana: process.env.SOLANA_PAYER_KEYPAIR || '',
    },
    payee: {
      eip155: process.env.EIP155_PAYEE_ADDRESS || '',
      solana: process.env.SOLANA_PAYEE_ADDRESS || '',
    },
  },
} as const;

export function getChainConfig(chain: 'eip155' | 'solana') {
  return config.chains[chain];
}

export function getWalletConfig(chain: 'eip155' | 'solana') {
  return {
    payer: config.wallets.payer[chain],
    payee: config.wallets.payee[chain],
  };
}
