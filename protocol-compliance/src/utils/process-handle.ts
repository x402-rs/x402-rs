import type { ChildProcess } from "node:child_process";
import { spawn } from "node:child_process";

export class ProcessHandle {
  private readonly process: ChildProcess;

  static spawn(...args: Parameters<typeof spawn>) {
    const childProcess = spawn(...args);
    childProcess.stdout?.on("data", (data) => {
      process.stdout.write(`[facilitator] ${data}`);
    });
    childProcess.stderr?.on("data", (data) => {
      process.stderr.write(`[facilitator] ${data}`);
    });
    childProcess.on("error", (err) => {
      console.error(`Facilitator error: ${err}`);
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
