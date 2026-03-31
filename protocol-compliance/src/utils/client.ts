import { x402Client, wrapFetchWithPayment } from "@x402/fetch";
import { ExactEvmScheme } from "@x402/evm/exact/client";
import { privateKeyToAccount } from "viem/accounts";
import { config } from "./config.js";
import { spawn } from "child_process";
import { WORKSPACE_ROOT } from "./workspace-root";
import { printLines } from "./process-handle";
import { ExactSvmScheme } from "@x402/svm/exact/client";
import { createKeyPairSignerFromBytes } from "@solana/kit";
import { base58 } from "@scure/base";
import { createPublicClient, createWalletClient, http } from "viem";
import { baseSepoliaPreconf } from "viem/chains";
import { ERC20_ABI, ERC20_APPROVE_ABI } from "./erc-abi";
import { UptoEvmSchemeClient } from "./upto-evm-scheme";

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
  } else if ("solana" in privateKeys) {
    env = {
      ...env,
      SOLANA_PRIVATE_KEY: privateKeys["solana"],
      SOLANA_RPC_URL: config.solanaDevnet.rpc,
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

export const EIP155_ACCOUNT = privateKeyToAccount(
  config.baseSepolia.buyerPrivateKey,
);

export async function makeFetch(
  chain: "eip155" | "solana",
): Promise<typeof fetch> {
  const client = new x402Client();
  switch (chain) {
    case "eip155": {
      client.register("eip155:*", new ExactEvmScheme(EIP155_ACCOUNT));
      client.register("eip155:*", new UptoEvmSchemeClient(EIP155_ACCOUNT));
      break;
    }
    case "solana": {
      const keypair = await createKeyPairSignerFromBytes(
        base58.decode(config.solanaDevnet.buyerPrivateKey),
      );
      client.register("solana:*", new ExactSvmScheme(keypair));
      break;
    }
    default:
      throw new Error(`Unsupported chain: ${chain}`);
  }
  return wrapFetchWithPayment(fetch, client);
}

// Create public client for Base Sepolia
export const PUBLIC_CLIENT = createPublicClient({
  chain: baseSepoliaPreconf,
  transport: http(),
});

// Create wallet client for signing transactions
export const WALLET_CLIENT = createWalletClient({
  account: EIP155_ACCOUNT,
  chain: baseSepoliaPreconf,
  transport: http(),
});

export async function setAllowance(
  tokenAddress: `0x${string}`,
  spender: `0x${string}`,
  amount: bigint,
) {
  const currentAllowance = await getAllowance(tokenAddress, spender);
  if (currentAllowance === amount) {
    return;
  }

  const approveTxHash = await WALLET_CLIENT.writeContract({
    address: tokenAddress,
    abi: ERC20_APPROVE_ABI,
    functionName: "approve",
    args: [spender, amount],
  });
  // Wait till the tx _lands_ onchain
  while (true) {
    const receipt = await PUBLIC_CLIENT.waitForTransactionReceipt({
      hash: approveTxHash,
      confirmations: 1,
    });
    if (
      receipt.blockHash !==
      "0x0000000000000000000000000000000000000000000000000000000000000000"
    ) {
      // Not a preconfirmation anymore
      break;
    }
  }
}

export function getAllowance(
  tokenAddress: `0x${string}`,
  spender: `0x${string}`,
): Promise<bigint> {
  return PUBLIC_CLIENT.readContract({
    address: tokenAddress,
    abi: ERC20_ABI,
    functionName: "allowance",
    args: [EIP155_ACCOUNT.address, spender],
  });
}

export function getBalance(
  tokenAddress: `0x${string}`,
  owner: `0x${string}`,
): Promise<bigint> {
  return PUBLIC_CLIENT.readContract({
    address: tokenAddress,
    abi: ERC20_ABI,
    functionName: "balanceOf",
    args: [owner],
  });
}
