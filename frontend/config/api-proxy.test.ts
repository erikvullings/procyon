// @vitest-environment node
import { createServer, type Server } from 'node:http';
import type { AddressInfo } from 'node:net';

import { createServer as createViteServer, type ViteDevServer } from 'vite';
import { afterEach, describe, expect, it } from 'vitest';

import { API_PREFIX, createApiProxyOptions, DEFAULT_BACKEND_ORIGIN } from './api-proxy';

let origin: Server | undefined;
let vite: ViteDevServer | undefined;

afterEach(async () => {
  await vite?.close();
  vite = undefined;
  await new Promise<void>((resolve) => {
    if (!origin?.listening) {
      resolve();
      return;
    }
    origin.closeAllConnections();
    origin.close(() => resolve());
  });
  origin = undefined;
});

/**
 * Starts a backend that opens an SSE stream, emits one event immediately and
 * then holds the connection open indefinitely.
 *
 * This is the shape that catches a buffering proxy: a proxy that waits for the
 * response to complete before forwarding it will never deliver the first event,
 * because the response never completes.
 */
async function startHoldingSseOrigin(): Promise<string> {
  origin = createServer((request, response) => {
    if (request.url !== `${API_PREFIX}/v1/events`) {
      response.writeHead(404).end();
      return;
    }
    response.writeHead(200, {
      'content-type': 'text/event-stream',
      'cache-control': 'no-cache',
      connection: 'keep-alive',
    });
    response.write('event: runtime.ready\nid: 1\ndata: {"ok":true}\n\n');
    // Deliberately no `response.end()`.
  });

  await new Promise<void>((resolve, reject) => {
    origin?.once('error', reject);
    origin?.listen(0, '127.0.0.1', () => {
      origin?.off('error', reject);
      resolve();
    });
  });
  const { port } = origin.address() as AddressInfo;
  return `http://127.0.0.1:${port}`;
}

async function startViteProxyingTo(target: string): Promise<string> {
  vite = await createViteServer({
    configFile: false,
    root: new URL('..', import.meta.url).pathname,
    logLevel: 'silent',
    server: {
      host: '127.0.0.1',
      port: 0,
      proxy: { [API_PREFIX]: createApiProxyOptions(target) },
    },
  });
  await vite.listen();
  const address = vite.httpServer?.address() as AddressInfo;
  return `http://127.0.0.1:${address.port}`;
}

describe('createApiProxyOptions', () => {
  it('targets the backend and disables the timeouts that would kill an SSE stream', () => {
    const options = createApiProxyOptions(DEFAULT_BACKEND_ORIGIN);

    expect(options.target).toBe(DEFAULT_BACKEND_ORIGIN);
    expect(options.changeOrigin).toBe(true);
    expect(options.timeout).toBe(0);
    expect(options.proxyTimeout).toBe(0);
  });

  it('defaults to the loopback address the Axum host binds to', () => {
    expect(DEFAULT_BACKEND_ORIGIN).toBe('http://127.0.0.1:8787');
    expect(API_PREFIX).toBe('/api');
  });

  it('streams an SSE event through before the response completes', async (context) => {
    let target: string;
    try {
      target = await startHoldingSseOrigin();
    } catch (error: unknown) {
      if (
        error instanceof Error &&
        'code' in error &&
        (error.code === 'EACCES' || error.code === 'EPERM')
      ) {
        context.skip();
        return;
      }
      throw error;
    }
    const proxied = await startViteProxyingTo(target);

    const abort = new AbortController();
    const response = await fetch(`${proxied}${API_PREFIX}/v1/events`, {
      headers: { accept: 'text/event-stream' },
      signal: abort.signal,
    });

    expect(response.status).toBe(200);
    expect(response.headers.get('content-type')).toContain('text/event-stream');
    // Instructs any intermediary (nginx, and Vite's own compression) not to
    // buffer the stream.
    expect(response.headers.get('x-accel-buffering')).toBe('no');
    expect(response.headers.get('cache-control')).toContain('no-transform');

    const reader = response.body?.getReader();
    expect(reader).toBeDefined();

    const firstChunk = await Promise.race([
      reader?.read().then(({ value }) => new TextDecoder().decode(value)),
      new Promise<string>((_, reject) =>
        setTimeout(() => reject(new Error('timed out: the proxy buffered the stream')), 4000),
      ),
    ]);

    expect(firstChunk).toContain('event: runtime.ready');
    expect(firstChunk).toContain('data: {"ok":true}');

    abort.abort();
  }, 20_000);
});
