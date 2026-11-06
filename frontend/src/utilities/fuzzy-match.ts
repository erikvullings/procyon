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

/** A single character is a word boundary when it isn't an ASCII alnum — the cheap token separator
 * fzf-style scorers use (covers `_`, `-`, `.`, space, `/`, start-of-string, etc.). */
function isWordSeparator(char: string | undefined): boolean {
  return char === undefined || !/[a-z0-9]/i.test(char);
}

/**
 * Score of `query` against `name` treating `query` as a subsequence of `name`.
 *
 * Returns `undefined` when `query` is NOT a subsequence of `name` (no match). Otherwise a number
 * where **higher is better**:
 * - +100 per matched character;
 * - +60 when the very first query character matches at the start of the name (leading bonus);
 * - +40 when a matched character sits at a word boundary (so `fz` beats `x_f_z`);
 * - +30 burstiness when consecutive query characters land on consecutive name characters;
 * - a small gap penalty for jumps between non-consecutive matches.
 *
 * An empty query scores `0` against any name (matches everything equally, like a blank filter).
 */
export function fuzzyScore(query: string, name: string): number | undefined {
  const q = foldCase(query);
  if (q.length === 0) return 0;
  const n = foldCase(name);
  if (n.length === 0) return undefined;

  let score = 0;
  let qi = 0;
  let previousMatch = -2; // far enough back that the first real match isn't treated as consecutive

  for (let ni = 0; ni < n.length && qi < q.length; ni += 1) {
    if (n[ni] !== q[qi]) continue;
    const atWordStart = ni === 0 || isWordSeparator(n[ni - 1]);
    score += 100;
    if (qi === 0) score += 60; // leading bonus, only for the first query character
    if (atWordStart) score += 40;
    if (ni === previousMatch + 1)
      score += 30; // burstiness
    else if (previousMatch >= 0) score -= Math.min(ni - previousMatch - 1, 12) * 3; // gap penalty, capped
    previousMatch = ni;
    qi += 1;
  }
  return qi === q.length ? score : undefined;
}

/** True when `query` fuzzy-matches `name` (empty query matches everything). */
export function fuzzyMatches(query: string, name: string): boolean {
  return fuzzyScore(query, name) !== undefined;
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
