import dotenv from "dotenv";
import tmp from "tmp";
import * as fs from "node:fs/promises";

dotenv.config();

const BASE_SEPOLIA_RPC_URL = process.env.BASE_SEPOLIA_RPC_URL;
if (!BASE_SEPOLIA_RPC_URL) throw new Error("BASE_SEPOLIA_RPC_URL is required");
const BASE_SEPOLIA_BUYER_PRIVATE_KEY =
  process.env.BASE_SEPOLIA_BUYER_PRIVATE_KEY;
if (!BASE_SEPOLIA_BUYER_PRIVATE_KEY)
  throw new Error("BASE_SEPOLIA_BUYER_PRIVATE_KEY is required");
const BASE_SEPOLIA_FACILITATOR_PRIVATE_KEY =
  process.env.BASE_SEPOLIA_FACILITATOR_PRIVATE_KEY;
if (!BASE_SEPOLIA_FACILITATOR_PRIVATE_KEY)
  throw new Error("BASE_SEPOLIA_FACILITATOR_PRIVATE_KEY is required");

const SOLANA_DEVNET_RPC_URL = process.env.SOLANA_DEVNET_RPC_URL;
if (!SOLANA_DEVNET_RPC_URL)
  throw new Error("SOLANA_DEVNET_RPC_URL is required");
const SOLANA_DEVNET_BUYER_PRIVATE_KEY =
  process.env.SOLANA_DEVNET_BUYER_PRIVATE_KEY;
if (!SOLANA_DEVNET_BUYER_PRIVATE_KEY)
  throw new Error("SOLANA_DEVNET_BUYER_PRIVATE_KEY is required");
const SOLANA_DEVNET_FACILITATOR_PRIVATE_KEY =
  process.env.SOLANA_DEVNET_FACILITATOR_PRIVATE_KEY;
if (!SOLANA_DEVNET_FACILITATOR_PRIVATE_KEY)
  throw new Error("SOLANA_DEVNET_FACILITATOR_PRIVATE_KEY is required");

export const config = {
  baseSepolia: {
    rpc: BASE_SEPOLIA_RPC_URL,
    buyerPrivateKey: BASE_SEPOLIA_BUYER_PRIVATE_KEY as `0x${string}`,
    facilitatorPrivateKey:
      BASE_SEPOLIA_FACILITATOR_PRIVATE_KEY as `0x${string}`,
  },
  solanaDevnet: {
    rpc: SOLANA_DEVNET_RPC_URL,
    buyerPrivateKey: SOLANA_DEVNET_BUYER_PRIVATE_KEY,
    facilitatorPrivateKey: SOLANA_DEVNET_FACILITATOR_PRIVATE_KEY,
  },
} as const;

export const FACILITATOR_CONFIG = {
  host: "0.0.0.0",
  chains: {
    "eip155:84532": {
      _comment: "Base Sepolia",
      eip1559: true,
      flashblocks: true,
      signers: [config.baseSepolia.facilitatorPrivateKey],
      rpc: [
        {
          http: config.baseSepolia.rpc,
          rate_limit: 50,
        },
      ],
    },
    "solana:EtWTRABZaYq6iMfeYKouRu166VU2xqa1": {
      _comment: "Solana Devnet",
      signer: config.solanaDevnet.facilitatorPrivateKey,
      rpc: config.solanaDevnet.rpc,
    },
  },
  schemes: [
    {
      id: "v1-eip155-exact",
      chains: "eip155:*",
    },
    {
      id: "v2-eip155-exact",
      chains: "eip155:*",
    },
    {
      id: "v1-solana-exact",
      chains: "solana:*",
    },
    {
      id: "v2-solana-exact",
      chains: "solana:*",
    },
  ],
};

export async function makeFacilitatorConfig(): Promise<string> {
  const filename = await new Promise<string>((resolve, reject) => {
    tmp.file({ postfix: ".json" }, (err, path) => {
      if (err) {
        reject(err);
      } else {
        resolve(path);
      }
    });
  });
  await fs.writeFile(filename, JSON.stringify(FACILITATOR_CONFIG, null, 2));
  return filename;
}

export const TIMEOUT = 20000;
export const TEST_CONFIG = { timeout: TIMEOUT } as const;
