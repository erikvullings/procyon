import { describe, expect, it } from 'vitest';
import { buildMediaUrl } from './media-url';

const location = { providerId: 'local', uri: 'file:///music/a b.mp3' };

describe('buildMediaUrl', () => {
  it('builds a same-origin URL carrying the JSON location', () => {
    const url = new URL(buildMediaUrl('', location, undefined), 'http://x.test');
    expect(url.pathname).toBe('/api/v1/media');
    expect(JSON.parse(url.searchParams.get('location') ?? '')).toEqual(location);
    expect(url.searchParams.has('token')).toBe(false);
  });

  it('prefixes the configured base URL and appends the session token', () => {
    const built = buildMediaUrl('http://127.0.0.1:8787', location, 'secret');
    expect(built.startsWith('http://127.0.0.1:8787/api/v1/media?')).toBe(true);
    expect(new URL(built).searchParams.get('token')).toBe('secret');
  });
});
