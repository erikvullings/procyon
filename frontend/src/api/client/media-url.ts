import type { Location } from '../../models/location';

/**
 * Builds the `GET /api/v1/media` URL for a seekable media element (task 0260). Media elements
 * cannot send an `Authorization` header, so the session token travels as `?token=` — the same
 * convention the SSE stream uses.
 */
export function buildMediaUrl(
  baseUrl: string,
  location: Location,
  token: string | undefined,
): string {
  const url = new URL('/api/v1/media', baseUrl.length > 0 ? baseUrl : 'http://placeholder.invalid');
  url.searchParams.set(
    'location',
    JSON.stringify({ providerId: location.providerId, uri: location.uri }),
  );
  if (token !== undefined && token.length > 0) url.searchParams.set('token', token);
  return baseUrl.length > 0 ? url.toString() : `${url.pathname}${url.search}`;
}
