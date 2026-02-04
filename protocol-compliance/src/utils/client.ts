import { x402Client, wrapFetchWithPayment } from "@x402/fetch";
import { registerExactEvmScheme } from "@x402/evm/exact/client";
import { privateKeyToAccount } from "viem/accounts";
import { config } from "./config.js";
import { spawn } from "child_process";
import { WORKSPACE_ROOT } from "./workspace-root";
import { printLines } from "./process-handle";
import { registerExactSvmScheme } from "@x402/svm/exact/client";
import { createKeyPairSignerFromBytes } from "@solana/kit";
import { base58 } from "@scure/base";

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
  } else if ("solana" in privateKeys) {
    env = {
      ...env,
      SOLANA_PRIVATE_KEY: privateKeys["solana"],
      SOLANA_RPC_URL: config.chains.solana.rpcUrl,
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

export async function makeFetch(
  chain: "eip155" | "solana",
): Promise<typeof fetch> {
  const client = new x402Client();
  switch (chain) {
    case "eip155": {
      let signer = privateKeyToAccount(
        config.wallets.payer.eip155 as `0x${string}`,
      );
      registerExactEvmScheme(client, { signer });
      break;
    }
    case "solana": {
      const keypair = await createKeyPairSignerFromBytes(
        base58.decode(config.wallets.payer.solana),
      );
      registerExactSvmScheme(client, { signer: keypair });
      break;
    }
    default:
      throw new Error(`Unsupported chain: ${chain}`);
  }
  return wrapFetchWithPayment(fetch, client);
}
