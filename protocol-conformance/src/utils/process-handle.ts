import type { ChildProcess } from "node:child_process";
import { spawn } from "node:child_process";

const TEXT_DECODER = new TextDecoder()

export function printLines(stream: {write: (s: string | Uint8Array) => void}, prefix: string, data: any) {
  if (data instanceof Uint8Array) {
    data = TEXT_DECODER.decode(data)
  }
  if (typeof data === "string") {
    const lines = data.split("\n");
    // stream.write(`${prefix} ${lines}`);
    for (const line of lines) {
      if (line) {
        stream.write(`${prefix} ${line}\n`);
      }
    }
    return
  }
    stream.write(`${prefix} ${data}`);
}

export class ProcessHandle {
  private readonly process: ChildProcess;

  static spawn(role: string,...args: Parameters<typeof spawn>) {
    const childProcess = spawn(...args);
    const prefix = `[${role}]`
    childProcess.stdout?.on("data", (data) => {
      printLines(process.stdout, prefix, data)
    });
    childProcess.stderr?.on("data", (data) => {
      if (typeof data === "string") {
        const lines = data.split("\n");
        for (const line of lines) {
          process.stderr.write(`${prefix} ${line}`);
        }
      } else {
        process.stderr.write(`${prefix} ${data}`);
      }
    });
    childProcess.on("error", (err) => {
      console.error(`${prefix} ERROR: ${err}`);
    });
    return new ProcessHandle(childProcess);
  }

  constructor(process: ChildProcess) {
    this.process = process;
  }

  waitExit() {
    const exitP = Promise.withResolvers<void>();
    const onError = (err: Error) => {
      this.process.off("exit", onExit);
      exitP.reject(err);
    }
    const onExit = () => {
      this.process.off("error", onError);
      exitP.resolve();
    }
    this.process.on("error", onError)
    this.process.on("exit", onExit);
    return exitP.promise;
  }

  async stop(): Promise<void> {
    const stopP = Promise.withResolvers<void>();
    this.process.kill("SIGTERM");
    this.process.on("exit", () => {
      stopP.resolve();
    });
    return stopP.promise;
  }
}
