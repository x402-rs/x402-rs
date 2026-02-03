import dotenv from 'dotenv';

dotenv.config();

export interface TestConfig {
  facilitator: {
    url: string;
    port: number;
  };
  server: {
    port: number;
  };
  chains: {
    eip155: {
      rpcUrl: string;
      network: string;
    };
    solana: {
      rpcUrl: string;
      network: string;
    };
    aptos: {
      rpcUrl: string;
      network: string;
    };
  };
  wallets: {
    payer: {
      eip155: string;
      solana: string;
      aptos: string;
    };
    payee: {
      eip155: string;
      solana: string;
      aptos: string;
    };
  };
}

export const config: TestConfig = {
  facilitator: {
    url: process.env.FACILITATOR_URL || 'http://localhost:23635',
    port: parseInt(process.env.FACILITATOR_PORT || '23635', 10),
  },
  server: {
    port: parseInt(process.env.SERVER_PORT || '3000', 10),
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
    aptos: {
      rpcUrl: process.env.APTOS_RPC_URL || 'https://fullnode.devnet.aptoslabs.com/v1',
      network: 'devnet',
    },
  },
  wallets: {
    payer: {
      eip155: process.env.EVM_PAYER_PRIVATE_KEY || '',
      solana: process.env.SOLANA_PAYER_KEYPAIR || '',
      aptos: process.env.APTOS_PAYER_PRIVATE_KEY || '',
    },
    payee: {
      eip155: process.env.EIP155_PAYEE_ADDRESS || '',
      solana: process.env.SOLANA_PAYEE_ADDRESS || '',
      aptos: process.env.APTOS_PAYEE_ADDRESS || '',
    },
  },
};

export function getChainConfig(chain: 'eip155' | 'solana' | 'aptos') {
  return config.chains[chain];
}

export function getWalletConfig(chain: 'eip155' | 'solana' | 'aptos') {
  return {
    payer: config.wallets.payer[chain],
    payee: config.wallets.payee[chain],
  };
}
