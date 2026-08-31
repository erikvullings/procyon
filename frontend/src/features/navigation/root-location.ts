import type { Location } from '../../models';

const SCHEME_AND_HOST = /^([a-z][a-z0-9+.-]*:\/\/[^/]*)\/?/iu;

/**
 * Derives "the root" of `location`'s filesystem for Ctrl+Backspace (Total
 * Commander parity, task 0128): the scheme-and-host prefix of the URI with a
 * single trailing slash, e.g. `file:///a/b/c` -> `file:///`,
 * `sftp://connection-1/home/user` -> `sftp://connection-1/`.
 *
 * This is a URI-prefix convention, not provider-aware: for remote providers
 * whose actual browsable root is a configured start path rather than `/`
 * (see `remoteRootLocation` in `connections-model.ts`), it lands one level
 * higher than that start path. Good enough for "go to the top of this tree"
 * without needing a `Connection` object, which Ctrl+Backspace's caller
 * (a bare pane cursor location) doesn't have available.
 */
export function rootLocationFor(location: Location): Location {
  const match = SCHEME_AND_HOST.exec(location.uri);
  const prefix = match?.[1];
  return { providerId: location.providerId, uri: prefix === undefined ? '/' : `${prefix}/` };
}
