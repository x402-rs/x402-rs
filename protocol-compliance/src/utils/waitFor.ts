export async function waitFor(
  predicate: () => Promise<boolean> | boolean,
  options: { timeoutMs?: number; intervalMs?: number } = {}
): Promise<boolean> {
  const timeoutMs = options.timeoutMs ?? 30000;
  const intervalMs = options.intervalMs ?? 500;

  const startTime = Date.now();

  while (Date.now() - startTime < timeoutMs) {
    if (await predicate()) {
      return true;
    }
    await new Promise(resolve => setTimeout(resolve, intervalMs));
  }

  return false;
}

export async function waitForUrl(
  url: string,
  options: { timeoutMs?: number; intervalMs?: number } = {}
): Promise<boolean> {
  return waitFor(
    async () => {
      try {
        const response = await fetch(url, { method: 'HEAD' });
        return response.ok;
      } catch {
        return false;
      }
    },
    options
  );
}

export function delay(ms: number): Promise<void> {
  return new Promise(resolve => setTimeout(resolve, ms));
}
