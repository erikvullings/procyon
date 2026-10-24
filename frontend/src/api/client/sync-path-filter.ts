/**
 * Include/exclude path filter for sync plans - the in-process mirror of
 * `fm_comparison::PathFilter` (task 0161) used by the mock client. Real hosts filter in Rust.
 *
 * `*` matches within a segment, `**` crosses `/`, `?` matches one non-`/` character. A pattern
 * without `/` matches any path segment; one with `/` matches from the compared root. A path is
 * matched when it or any ancestor matches, and exclusions win over inclusions.
 */
function globMatch(pattern: string, text: string): boolean {
  const match = (p: number, t: number): boolean => {
    if (p === pattern.length) return t === text.length;
    const char = pattern[p];
    if (char === '*') {
      const crosses = pattern[p + 1] === '*';
      const next = p + (crosses ? 2 : 1);
      for (let taken = t; taken <= text.length; taken += 1) {
        if (!crosses && text.slice(t, taken).includes('/')) break;
        if (match(next, taken)) return true;
      }
      return false;
    }
    if (t >= text.length) return false;
    if (char === '?') return text[t] !== '/' && match(p + 1, t + 1);
    return char === text[t] && match(p + 1, t + 1);
  };
  return match(0, 0);
}

function patternMatchesPath(pattern: string, relativePath: string): boolean {
  const segments = relativePath.split('/');
  const wholePath = pattern.includes('/');
  for (let end = 1; end <= segments.length; end += 1) {
    const candidate = wholePath ? segments.slice(0, end).join('/') : segments[end - 1];
    if (candidate !== undefined && globMatch(pattern, candidate)) return true;
  }
  return false;
}

function clean(patterns: readonly string[] | undefined): string[] {
  return (patterns ?? [])
    .map((pattern) => pattern.trim().replace(/^\.\//u, ''))
    .filter((pattern) => pattern !== '');
}

/** Whether `relativePath` survives the include/exclude patterns. */
export function admitsPath(
  relativePath: string,
  include: readonly string[] | undefined,
  exclude: readonly string[] | undefined,
): boolean {
  const excluded = clean(exclude);
  if (excluded.some((pattern) => patternMatchesPath(pattern, relativePath))) return false;
  const included = clean(include);
  return included.length === 0 || included.some((p) => patternMatchesPath(p, relativePath));
}
