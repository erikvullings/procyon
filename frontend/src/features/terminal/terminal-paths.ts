/**
 * Paths for typing into a terminal (task 0333). An entry's location URI is turned into the
 * absolute path the shell on that host understands, and quoted so the shell reads it as one word.
 */

/**
 * The absolute path of a `file://` or `sftp://<connection>/` location, or `undefined` for any other
 * scheme or a malformed URI.
 */
export function absolutePathOf(uri: string): string | undefined {
  try {
    if (uri.startsWith('file://')) return decodeURIComponent(uri.slice('file://'.length));
    const remote = /^sftp:\/\/[^/]+(\/.*)?$/.exec(uri);
    if (remote !== null) return decodeURIComponent(remote[1] ?? '/');
    return undefined;
  } catch {
    return undefined;
  }
}

/**
 * Quotes a path for bash, zsh and fish alike: plain paths stay as they are; anything else is
 * single-quoted, with each embedded `'` written as `'\''`.
 */
export function shellQuote(path: string): string {
  if (/^[A-Za-z0-9_./:@%+,=-]+$/.test(path)) return path;
  return `'${path.replace(/'/g, "'\\''")}'`;
}
