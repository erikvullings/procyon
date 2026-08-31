import type { ProxyOptions } from 'vite';

/** Path prefix every backend route lives under (specification §8). */
export const API_PREFIX = '/api';

/**
 * Where `fm-server` listens by default.
 *
 * The backend binds to loopback only (specification §22). Task 0008 must use
 * this port, or `FM_SERVER_ORIGIN` must be set for both processes.
 */
export const DEFAULT_BACKEND_ORIGIN = 'http://127.0.0.1:8787';

function isEventStream(contentType: string | string[] | undefined): boolean {
  const value = Array.isArray(contentType) ? contentType.join(',') : (contentType ?? '');
  return value.includes('text/event-stream');
}

/**
 * Builds the dev-server proxy for `/api`.
 *
 * Plain REST calls need nothing special, but the SSE endpoint
 * (`GET /api/v1/events`, task 0032) is a long-lived response that is never
 * "complete", so the proxy has to be told three things:
 *
 * 1. Do not time out — the default socket and proxy timeouts would sever an
 *    idle event stream.
 * 2. Do not negotiate a compressed response upstream. A compression stream
 *    buffers until its internal block is full, which delays events
 *    indefinitely on a low-traffic stream.
 * 3. Do not let anything downstream buffer or transform the response, and
 *    flush the headers immediately so the browser opens the stream rather than
 *    waiting for the first byte of the body.
 */
export function createApiProxyOptions(target: string): ProxyOptions {
  return {
    target,
    changeOrigin: true,
    timeout: 0,
    proxyTimeout: 0,
    configure: (proxy) => {
      proxy.on('proxyReq', (proxyReq, request) => {
        if (isEventStream(request.headers.accept)) {
          proxyReq.setHeader('accept-encoding', 'identity');
          proxyReq.setHeader('connection', 'keep-alive');
        }
      });

      proxy.on('proxyRes', (proxyRes, _request, response) => {
        if (!isEventStream(proxyRes.headers['content-type'])) {
          return;
        }
        // These are set on the *upstream* header bag rather than on `response`.
        // The proxy copies `proxyRes.headers` onto the response after this
        // event fires, so anything written directly to `response` here is
        // overwritten — and flushing here is worse still, because the copy is
        // guarded by `if (!res.headersSent)` and would be skipped entirely,
        // losing `content-type`.
        proxyRes.headers['cache-control'] = 'no-cache, no-transform';
        proxyRes.headers['x-accel-buffering'] = 'no';
        // Safe once the copy has run: opens the stream for the client even if
        // the backend has not sent its first event yet.
        setImmediate(() => response.flushHeaders());
      });
    },
  };
}
