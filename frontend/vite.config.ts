import { mithrilInspector } from '@mithril-inspector/vite';
import { defineConfig } from 'vite';

import { API_PREFIX, createApiProxyOptions, DEFAULT_BACKEND_ORIGIN } from './config/api-proxy.ts';

const backendOrigin = process.env.FM_SERVER_ORIGIN ?? DEFAULT_BACKEND_ORIGIN;

export default defineConfig({
  plugins: [
    mithrilInspector({
      editor: 'code',
      mode: 'full',
      ui: {
        theme: 'system',
      },
    }),
  ],
  server: {
    host: '127.0.0.1',
    port: 5180,
    strictPort: true,
    proxy: {
      [API_PREFIX]: createApiProxyOptions(backendOrigin),
    },
  },
  // Lazily imported map libraries would otherwise be discovered mid-session, triggering a
  // re-optimisation that 504s ("Outdated Optimize Dep") the already-requested MapLibre worker.
  optimizeDeps: {
    include: ['maplibre-gl', 'terra-draw', 'terra-draw-maplibre-gl-adapter', 'pmtiles'],
  },
  build: {
    target: 'es2023',
    sourcemap: true,
  },
});
