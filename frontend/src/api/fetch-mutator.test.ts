import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  ApiError,
  fetchMutator,
  setBaseUrlOverride,
  setSessionHeaderProvider,
  setUnauthorizedHandler,
} from './fetch-mutator';

function jsonResponse(
  status: number,
  body: unknown,
  headers: Record<string, string> = {},
): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json', ...headers },
  });
}

afterEach(() => {
  setBaseUrlOverride(undefined);
  setSessionHeaderProvider(undefined);
  setUnauthorizedHandler(undefined);
  vi.restoreAllMocks();
});

describe('fetchMutator', () => {
  it('resolves with the parsed JSON body, status and headers for a successful response', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(jsonResponse(200, { status: 'ok' })));

    const result = await fetchMutator<{
      status: number;
      data: { status: string };
      headers: Headers;
    }>('/api/v1/health', {});

    expect(result.status).toBe(200);
    expect(result.data).toEqual({ status: 'ok' });
    expect(result.headers).toBeInstanceOf(Headers);
  });

  it('preserves binary image bytes without UTF-8 text decoding', async () => {
    const bytes = new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0xff, 0x00]);
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(bytes, {
          status: 200,
          headers: { 'content-type': 'image/png' },
        }),
      ),
    );

    const result = await fetchMutator<{
      status: number;
      data: Blob;
      headers: Headers;
    }>('/api/v1/icons?uri=file%3A%2F%2F%2Freport.pdf');

    expect(new Uint8Array(await result.data.arrayBuffer())).toEqual(bytes);
  });

  it('keeps SVG icon-theme assets as text for sanitization and rendering', async () => {
    const svg = '<svg viewBox="0 0 16 16"><path d="M1 1h2v2z" /></svg>';
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response(svg, {
          status: 200,
          headers: { 'content-type': 'image/svg+xml' },
        }),
      ),
    );

    const result = await fetchMutator<{ status: number; data: string; headers: Headers }>(
      '/api/v1/plugins/catppuccin.icons/icon-theme/asset?path=icons%2Ffolder.svg',
    );

    expect(result.data).toBe(svg);
  });

  it('prefixes the request URL with the configured base URL override', async () => {
    const fetchSpy = vi.fn().mockResolvedValue(jsonResponse(200, { status: 'ok' }));
    vi.stubGlobal('fetch', fetchSpy);
    setBaseUrlOverride('http://example.test');

    await fetchMutator('/api/v1/health', {});

    expect(fetchSpy).toHaveBeenCalledWith('http://example.test/api/v1/health', expect.anything());
  });

  it('attaches the configured session header to every request', async () => {
    const fetchSpy = vi.fn().mockResolvedValue(jsonResponse(200, { status: 'ok' }));
    vi.stubGlobal('fetch', fetchSpy);
    setSessionHeaderProvider(() => ({
      name: 'Authorization',
      value: 'Bearer token123',
    }));

    await fetchMutator('/api/v1/health', {});

    const [, options] = fetchSpy.mock.calls[0] as [string, RequestInit];
    const headers = new Headers(options.headers);
    expect(headers.get('Authorization')).toBe('Bearer token123');
  });

  it('invokes the unauthorized handler on a 401 response before throwing', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        jsonResponse(401, {
          code: 'unauthorized',
          message: 'A valid session token is required.',
        }),
      ),
    );
    const handler = vi.fn();
    setUnauthorizedHandler(handler);

    await expect(fetchMutator('/api/v1/workspaces', {})).rejects.toMatchObject({ status: 401 });

    expect(handler).toHaveBeenCalledOnce();
  });

  it('does not invoke the unauthorized handler for other error statuses', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(jsonResponse(403, { code: 'forbidden', message: 'Forbidden.' })),
    );
    const handler = vi.fn();
    setUnauthorizedHandler(handler);

    await expect(fetchMutator('/api/v1/workspaces', {})).rejects.toMatchObject({ status: 403 });

    expect(handler).not.toHaveBeenCalled();
  });

  it('maps a non-2xx JSON error body into a typed ApiError', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        jsonResponse(409, {
          code: 'destinationAlreadyExists',
          message: 'A file named report.pdf already exists.',
          requestId: 'e1ce66cc-64a8-4ae7-9cc1-2882bc80de4e',
          details: { destination: 'file:///Users/erik/Documents/report.pdf' },
        }),
      ),
    );

    await expect(fetchMutator('/api/v1/entries/metadata', {})).rejects.toMatchObject({
      name: 'ApiError',
      code: 'destinationAlreadyExists',
      message: 'A file named report.pdf already exists.',
      requestId: 'e1ce66cc-64a8-4ae7-9cc1-2882bc80de4e',
      details: { destination: 'file:///Users/erik/Documents/report.pdf' },
      status: 409,
    });
  });

  it('falls back to a generic ApiError when the error body has no code/message shape', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn().mockResolvedValue(
        new Response('internal error', {
          status: 500,
          statusText: 'Internal Server Error',
        }),
      ),
    );

    let error: ApiError | undefined;
    try {
      await fetchMutator('/api/v1/health', {});
    } catch (caught) {
      error = caught as ApiError;
    }

    expect(error).toBeInstanceOf(ApiError);
    expect(error?.code).toBe('unknownError');
    expect(error?.status).toBe(500);
  });

  it('forwards the caller-provided AbortSignal to the underlying fetch call', async () => {
    const fetchSpy = vi.fn().mockResolvedValue(jsonResponse(200, { status: 'ok' }));
    vi.stubGlobal('fetch', fetchSpy);
    const controller = new AbortController();

    await fetchMutator('/api/v1/health', { signal: controller.signal });

    const [, options] = fetchSpy.mock.calls[0] as [string, RequestInit];
    expect(options.signal).toBe(controller.signal);
  });

  it('propagates an abort rejection without wrapping it in ApiError', async () => {
    const abortError = new DOMException('The operation was aborted.', 'AbortError');
    vi.stubGlobal('fetch', vi.fn().mockRejectedValue(abortError));

    await expect(fetchMutator('/api/v1/health', {})).rejects.toBe(abortError);
  });
});
