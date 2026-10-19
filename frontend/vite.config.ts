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
  // MapLibre locates its worker with `new URL('./maplibre-gl-worker.mjs', import.meta.url)`; once
  // pre-bundled into .vite/deps that sibling file is missing and the dev server answers 504
  // ("Outdated Optimize Dep"). Serve it straight from node_modules instead.
  optimizeDeps: {
    exclude: ['maplibre-gl'],
    include: ['terra-draw', 'terra-draw-maplibre-gl-adapter', 'pmtiles'],
  },
  build: {
    target: 'es2023',
    sourcemap: true,
  },
});
