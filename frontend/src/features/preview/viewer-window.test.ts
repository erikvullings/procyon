import { afterEach, describe, expect, it, vi } from 'vitest';

import type { EntrySummary } from '../../models';
import {
  decodeViewerRequest,
  encodeViewerRequest,
  openViewerWindow,
  VIEWER_QUERY_PARAM,
  type ViewerWindowRequest,
} from './viewer-window';

const entry: EntrySummary = {
  id: 'entry-1',
  location: { providerId: 'local', uri: 'file:///tmp/Zürich %20 & café?.db' },
  name: 'Zürich & café?.db',
  kind: 'file',
  hidden: false,
  readOnly: false,
  metadataRevision: 1,
};

const request: ViewerWindowRequest = {
  entry,
  workspaceId: 'workspace-1',
  initialSearch: { query: 'ñ', regex: false, caseSensitive: true, wholeWord: false },
  initialPage: 3,
};

afterEach(() => {
  vi.restoreAllMocks();
});

describe('viewer window request', () => {
  it('round-trips unicode requests through a URL-safe token', () => {
    const token = encodeViewerRequest(request);
    expect(token).toMatch(/^[A-Za-z0-9_-]+$/);
    expect(decodeViewerRequest(`?${VIEWER_QUERY_PARAM}=${token}`)).toEqual(request);
  });

  it('drops bulky transient entry fields so the token stays small', () => {
    const bulky = {
      ...entry,
      contentMatches: Array.from({ length: 500 }, (_, index) => ({
        lineNumber: index,
        offset: 0,
        length: 80,
        preview: 'x'.repeat(80),
      })),
    } as unknown as EntrySummary;
    const token = encodeViewerRequest({ entry: bulky });
    expect(token.length).toBeLessThan(1024);
    expect(decodeViewerRequest(`?${VIEWER_QUERY_PARAM}=${token}`)?.entry).toEqual(entry);
  });

  it.each([
    '',
    '?other=1',
    `?${VIEWER_QUERY_PARAM}=`,
    `?${VIEWER_QUERY_PARAM}=not-base64!!`,
    `?${VIEWER_QUERY_PARAM}=${btoa('not json')}`,
    `?${VIEWER_QUERY_PARAM}=${btoa(JSON.stringify({ entry: { id: 1 } }))}`,
    `?${VIEWER_QUERY_PARAM}=${btoa(JSON.stringify({ entry: { ...entry, kind: 'directory' } }))}`,
  ])('rejects the malformed query %j', (search) => {
    expect(decodeViewerRequest(search)).toBeUndefined();
  });
});

describe('openViewerWindow', () => {
  it('uses the desktop window command when the client has one', async () => {
    const openViewerWindowCommand = vi.fn().mockResolvedValue(undefined);
    await expect(
      openViewerWindow({ openViewerWindow: openViewerWindowCommand }, request),
    ).resolves.toBe(true);
    expect(openViewerWindowCommand).toHaveBeenCalledWith(encodeViewerRequest(request), entry.name);
  });

  it('reports failure when the desktop command fails so the caller can fall back', async () => {
    const failing = vi.fn().mockRejectedValue(new Error('no window'));
    await expect(openViewerWindow({ openViewerWindow: failing }, request)).resolves.toBe(false);
  });

  it('opens a popup of the same page in the browser and reports blocked popups', async () => {
    const open = vi.spyOn(globalThis, 'open').mockReturnValue({} as Window);
    await expect(openViewerWindow({}, request)).resolves.toBe(true);
    const [url, target] = open.mock.calls[0] ?? [];
    expect(String(url)).toContain(`?${VIEWER_QUERY_PARAM}=${encodeViewerRequest(request)}`);
    expect(target).toBe('_blank');

    open.mockReturnValue(null);
    await expect(openViewerWindow({}, request)).resolves.toBe(false);
  });
});
