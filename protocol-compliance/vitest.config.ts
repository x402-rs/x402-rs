import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    globals: true,
    include: ['src/tests/**/*.test.ts'],
    exclude: ['node_modules'],
    // Run tests in series (one file at a time) to avoid port conflicts
    run: true,
  },
});
