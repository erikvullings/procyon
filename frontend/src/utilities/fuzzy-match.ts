/**
 * Small, stable fuzzy (subsequence) matcher shared by the quick filter and keyboard
 * type-to-select (task 0310). Both call the *same* functions, which is what guarantees the cursor
 * and the filter text can never disagree. Pure and deterministic — no I/O, no clock.
 */

/** Case- and diacritic-insensitive fold: NFC-decompose, drop combining marks, lowercase. So
 * `Café` matches `cafe`, `Müller` matches `mueller`. */
export function foldCase(input: string): string {
  return input
    .normalize('NFD')
    .replace(/[\u0300-\u036f]/gu, '')
    .toLowerCase();
}

/** Split a folded name into alphanumeric tokens (any run of non-alphanumeric characters — spaces,
 * `_`, `.`, `-`, `/`, … — is a boundary). Fuzzy matching is confined to word tokens so a query
 * like `math` matches `mathematics` but not a name whose letters are scattered across unrelated
 * words; a multi-word query (e.g. `file manager`) still matches token-by-token in order. */
function foldedTokens(name: string): string[] {
  const tokens: string[] = [];
  let current = '';
  for (const ch of name) {
    if ((ch >= 'a' && ch <= 'z') || (ch >= '0' && ch <= '9')) current += ch;
    else if (current.length > 0) {
      tokens.push(current);
      current = '';
    }
  }
  if (current.length > 0) tokens.push(current);
  return tokens;
}

/** Score `query` as a subsequence of a single folded `token`, or `undefined` if it isn't one.
 * Higher is better: base per match, plus leading / token-start / consecutive (burstiness) bonuses
 * and a small capped gap penalty. */
function scoreTokenSubsequence(query: string, token: string): number | undefined {
  if (query.length === 0) return 0;
  if (token.length === 0) return undefined;
  let score = 0;
  let qi = 0;
  let previous = -2; // far enough back that the first match isn't treated as consecutive
  for (let ti = 0; ti < token.length && qi < query.length; ti += 1) {
    if (token[ti] !== query[qi]) continue;
    score += 100;
    if (qi === 0) score += 60; // leading bonus
    if (ti === 0) score += 40; // token-start bonus
    if (ti === previous + 1)
      score += 30; // burstiness
    else if (previous >= 0) score -= Math.min(ti - previous - 1, 12) * 3; // gap penalty
    previous = ti;
    qi += 1;
  }
  return qi === query.length ? score : undefined;
}

/**
 * Score of `query` against `name` treating `query` as a **token-wise subsequence**: `query` is
 * split on separators into word tokens, and each must appear (as a subsequence) in a distinct
 * name token, in order. So `entry-99999` matches `entry-99999` and `file manager` matches
 * `file_manager`, but `math` still rejects a name whose letters are scattered across unrelated
 * words (the old whole-name subsequence matched far too much). Returns `undefined` when no such
 * alignment exists; otherwise a number where **higher is better** (the sum of the per-token
 * scores, plus a bonus for query tokens landing in adjacent name tokens). An empty query scores `0`.
 */
export function fuzzyScore(query: string, name: string): number | undefined {
  const q = foldCase(query);
  if (q.length === 0) return 0;
  const queryTokens = foldedTokens(q);
  // A non-empty query made only of separators (e.g. a lone space from a type-to-select keystroke)
  // has no token to match, so it matches nothing - otherwise every entry would "match" vacuously.
  if (queryTokens.length === 0) return undefined;
  const n = foldCase(name);
  if (n.length === 0) return undefined;
  const nameTokens = foldedTokens(n);
  let score = 0;
  let qi = 0;
  let lastNameToken = -10;
  for (let ni = 0; ni < nameTokens.length; ni += 1) {
    if (qi >= queryTokens.length) break;
    const nameToken = nameTokens[ni];
    const queryToken = queryTokens[qi];
    if (nameToken === undefined || queryToken === undefined) continue;
    const sub = scoreTokenSubsequence(queryToken, nameToken);
    if (sub === undefined) continue;
    score += sub;
    if (ni === lastNameToken + 1) score += 20; // query tokens in adjacent name tokens
    lastNameToken = ni;
    qi += 1;
  }
  return qi === queryTokens.length ? score : undefined;
}

/** True when `query` fuzzy-matches `name` as a token-wise subsequence (see {@link fuzzyScore});
 * an empty query matches everything. */
export function fuzzyMatches(query: string, name: string): boolean {
  return fuzzyScore(query, name) !== undefined;
}

/** True when `query` appears as a contiguous, case/diacritic-insensitive substring of `name` —
 * the pre-0310 "contains" behaviour, used by the quick filter's `contains` match mode. */
export function substringMatches(query: string, name: string): boolean {
  const q = foldCase(query);
  if (q.length === 0) return true;
  return foldCase(name).includes(q);
}

/**
 * The single best fuzzy match for `query` among `items`, or `undefined` if none match. Ties keep
 * the earliest item (stable), so type-to-select is deterministic.
 */
export function bestFuzzyMatch<T>(
  query: string,
  items: readonly T[],
  nameOf: (item: T) => string,
): T | undefined {
  let best: T | undefined;
  let bestScore = Number.NEGATIVE_INFINITY;
  for (const item of items) {
    const score = fuzzyScore(query, nameOf(item));
    if (score === undefined) continue;
    if (score > bestScore) {
      bestScore = score;
      best = item;
    }
  }
  return best;
}

/**
 * `items` whose name fuzzy-matches `query`, ordered best-first (stable on ties). Used by
 * type-to-select's in-match cursor movement.
 */
export function rankFuzzyMatches<T>(
  query: string,
  items: readonly T[],
  nameOf: (item: T) => string,
): T[] {
  return items
    .map((item, index) => ({ item, index, score: fuzzyScore(query, nameOf(item)) }))
    .filter(
      (entry): entry is { item: T; index: number; score: number } => entry.score !== undefined,
    )
    .sort((a, b) => b.score - a.score || a.index - b.index)
    .map((entry) => entry.item);
}
