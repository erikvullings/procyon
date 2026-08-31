import { defineConfig } from 'vitest/config';

export default defineConfig({
  test: {
    // Component tests need a DOM; the proxy test opts into `node` with a
    // `@vitest-environment` docblock.
    environment: 'jsdom',
    include: ['src/**/*.test.ts', 'config/**/*.test.ts'],
    restoreMocks: true,
  },
});
