import getPort from "get-port";
import { waitForUrl } from "./waitFor.js";
import { WORKSPACE_ROOT } from "./workspace-root.js";
import { ProcessHandle } from "./process-handle.js";
import { makeFacilitatorConfig } from "./config.js";

export class RSFacilitatorHandle {
  readonly url: URL;
  readonly process: ProcessHandle;

  static async spawn(port?: number) {
    port = port ?? (await getPort());
    const facilitatorUrl = new URL(`http://localhost:${port}/`);

    const facilitatorBinary = new URL(
      "./target/debug/x402-facilitator",
      WORKSPACE_ROOT,
    ).pathname;

    // Start facilitator debug binary with absolute path
    console.log(
      `Starting facilitator ${facilitatorBinary} at ${facilitatorUrl}...`,
    );

    const configFilepath = await makeFacilitatorConfig();

    const facilitatorProcess = ProcessHandle.spawn(
      "rs-facilitator",
      facilitatorBinary,
      ["--config", configFilepath],
      {
        cwd: WORKSPACE_ROOT,
        stdio: ["ignore", "pipe", "pipe"],
        env: {
          ...process.env,
          PORT: port.toString(),
          // Ensure root .env is loaded - Rust dotenv looks for .env in cwd or parent dirs
          DOTENV_CONFIG: "full",
        },
      },
    );

    // Wait for facilitator to be ready
    const ready = await Promise.race([
      waitForUrl(facilitatorUrl, { timeoutMs: 60000 }),
      facilitatorProcess.waitExit(),
    ]);
    if (!ready) {
      throw new Error(`Facilitator failed to start within 60 seconds`);
    }

    console.log(`Facilitator started at ${facilitatorUrl}`);
    return new RSFacilitatorHandle(facilitatorUrl, facilitatorProcess);
  }

  constructor(url: URL, process: ProcessHandle) {
    this.url = url;
    this.process = process;
  }

  async stop() {
    await this.process.stop();
  }
}

export async function getSupportedChains(
  facilitatorUrl: string,
): Promise<string[]> {
  try {
    const response = await fetch(`${facilitatorUrl}/chains`);
    if (!response.ok) {
      throw new Error(`Failed to get chains: ${response.statusText}`);
    }
    return response.json();
  } catch {
    // If the chains endpoint doesn't exist, return default chains
    return ["eip155", "solana", "aptos"];
  }
}

export function isRemoteFacilitator(url: string): boolean {
  return (
    !url.startsWith("http://localhost") && !url.startsWith("http://127.0.0.1")
  );
}
