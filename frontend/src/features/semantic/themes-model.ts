import type {
  CorpusThemeStatus,
  CorpusThemes,
  DocumentThemeMixture,
  ThemeMapPoint,
} from '../../models';

/** Number of palette colours defined as `--fm-theme-N` custom properties in the theme sheet. */
export const THEME_PALETTE_SIZE = 8;

/** Neutral colour of the "no theme" bucket (a custom property, themed for light and dark). */
export const NO_THEME_COLOUR = 'var(--fm-theme-none)';

/**
 * Stable colour of a theme; `null` is the "no theme" bucket. Palette colours are
 * theme-sheet custom properties (colour-blind-safe Okabe–Ito base); further
 * themes cycle hues deterministically.
 */
export function themeColour(theme: number | null | undefined): string {
  if (theme === null || theme === undefined) return NO_THEME_COLOUR;
  if (theme < THEME_PALETTE_SIZE) return `var(--fm-theme-${theme})`;
  const hue = (theme * 137.508) % 360;
  return `hsl(${hue.toFixed(0)} 62% 46%)`;
}

/** Resolves a `var(--name)` colour against an element so a canvas can paint it. */
export function resolveColour(colour: string, element: Element): string {
  const match = /^var\((--[\w-]+)\)$/.exec(colour);
  if (match === null) return colour;
  const value = getComputedStyle(element)
    .getPropertyValue(match[1] ?? '')
    .trim();
  return value.length > 0 ? value : 'gray';
}

/**
 * Terms that are never a useful label: markup and URL debris from extracted text, and filler
 * words. The worker filters the same terms; repeating it here keeps themes computed by an older
 * worker readable.
 */
const NON_LABEL_TERMS: ReadonlySet<string> = new Set([
  'amp',
  'nbsp',
  'quot',
  'http',
  'https',
  'www',
  'com',
  'org',
  'html',
  'htm',
  'doi',
  'pdf',
  'may',
  'might',
  'must',
  'would',
  'could',
  'should',
  'shall',
  'one',
  'two',
  'use',
  'used',
  'using',
  'within',
  'such',
  'these',
  'those',
  'been',
  'being',
  'more',
  'most',
  'other',
  'some',
  'into',
  'between',
  'about',
  'after',
  'before',
  'during',
  'both',
  'each',
  'only',
  'very',
  'much',
  'many',
  'however',
  'per',
  'via',
  'see',
  'new',
  'et',
]);

/** Keywords fit to show, in their ranked order. */
export function usableKeywords<T extends { readonly term: string }>(
  keywords: readonly T[],
): readonly T[] {
  return keywords.filter((keyword) => !NON_LABEL_TERMS.has(keyword.term.toLowerCase()));
}

/** Label of a theme: its leading keywords, or a numbered fallback supplied by the caller. */
export function themeKeywordLabel(
  keywords: readonly { readonly term: string }[],
  limit = 3,
): string {
  return usableKeywords(keywords)
    .slice(0, limit)
    .map((keyword) => keyword.term)
    .join(', ');
}

/** True when the index changed after the themes were computed. */
export function themesAreStale(
  themes: Pick<CorpusThemes, 'indexRevision'>,
  status: Pick<CorpusThemeStatus, 'indexRevision'> | undefined,
): boolean {
  return status !== undefined && status.indexRevision !== themes.indexRevision;
}

export interface ScatterGeometry {
  readonly width: number;
  readonly height: number;
  readonly padding: number;
}

/** Maps a normalised coordinate in `[-1, 1]` (y up) to canvas pixels (y down). */
export function toScreen(
  point: { readonly x: number; readonly y: number },
  geometry: ScatterGeometry,
): { readonly px: number; readonly py: number } {
  const innerWidth = Math.max(1, geometry.width - 2 * geometry.padding);
  const innerHeight = Math.max(1, geometry.height - 2 * geometry.padding);
  return {
    px: geometry.padding + ((point.x + 1) / 2) * innerWidth,
    py: geometry.padding + ((1 - point.y) / 2) * innerHeight,
  };
}

/** Nearest point within `radius` pixels of a canvas position, or `undefined`. */
export function pickPoint(
  points: readonly ThemeMapPoint[],
  px: number,
  py: number,
  geometry: ScatterGeometry,
  radius: number,
): ThemeMapPoint | undefined {
  let best: ThemeMapPoint | undefined;
  let bestDistance = radius * radius;
  for (const point of points) {
    const screen = toScreen(point, geometry);
    const distance = (screen.px - px) ** 2 + (screen.py - py) ** 2;
    if (distance <= bestDistance) {
      best = point;
      bestDistance = distance;
    }
  }
  return best;
}

/**
 * One-based line to open a source at, taken from structural provenance.
 * JSONL records carry their absolute line; text and code chunks their first line.
 */
export function provenanceLine(provenance: string): number | undefined {
  let value: unknown;
  try {
    value = JSON.parse(provenance);
  } catch {
    return undefined;
  }
  const lineFrom = (candidate: unknown): number | undefined => {
    if (candidate === null || typeof candidate !== 'object' || Array.isArray(candidate)) {
      return undefined;
    }
    const fields = candidate as Record<string, unknown>;
    if (fields.kind === 'exact') return lineFrom(fields.value);
    if (fields.kind === 'span') return lineFrom(fields.first) ?? lineFrom(fields.last);
    const line =
      fields.kind === 'jsonlRecord' ? fields.line : (fields.start_line ?? fields.startLine);
    return typeof line === 'number' && Number.isInteger(line) && line > 0 ? line : undefined;
  };
  return lineFrom(value);
}

export interface ThemeDocumentShare {
  readonly sourceId: string;
  readonly title: string;
  readonly share: number;
  readonly chunkCount: number;
}

/** Documents that contribute most to a theme, strongest first, ties by title. */
export function topDocumentsForTheme(
  mixtures: readonly DocumentThemeMixture[],
  theme: number | null,
  limit: number,
  minShare = 0,
): readonly ThemeDocumentShare[] {
  const rows: ThemeDocumentShare[] = [];
  for (const mixture of mixtures) {
    const slice = mixture.shares.find((share) => (share.theme ?? null) === theme);
    if (slice !== undefined && slice.share >= minShare) {
      rows.push({
        sourceId: mixture.sourceId,
        title: mixture.title,
        share: slice.share,
        chunkCount: slice.chunkCount,
      });
    }
  }
  rows.sort(
    (left, right) =>
      right.chunkCount - left.chunkCount ||
      right.share - left.share ||
      left.title.localeCompare(right.title) ||
      left.sourceId.localeCompare(right.sourceId),
  );
  return rows.slice(0, limit);
}

/** Percentage text such as `42%`, `<1%` for tiny non-zero shares. */
export function percentText(share: number): string {
  if (share <= 0) return '0%';
  const percent = share * 100;
  return percent < 1 ? '<1%' : `${Math.round(percent)}%`;
}

/** True when the backend rejected a request because the analysis no longer matches the index. */
export function isThemesStale(error: unknown): boolean {
  if (typeof error !== 'object' || error === null) return false;
  const details = (error as { readonly details?: unknown }).details;
  return (
    typeof details === 'object' &&
    details !== null &&
    (details as { readonly reason?: unknown }).reason === 'corpusThemesStale'
  );
}

const NAMED_ENTITIES: Readonly<Record<string, string>> = {
  amp: '&',
  quot: '"',
  apos: "'",
  nbsp: ' ',
  lt: '<',
  gt: '>',
};

/**
 * Decodes the HTML entities extracted document text carries (`&amp;`, `&#39;`, …). With
 * `keepAngleBrackets` the `&lt;`/`&gt;` pair stays encoded so text bound for the Markdown
 * renderer can never turn into markup.
 */
export function decodeEntities(text: string, keepAngleBrackets = false): string {
  return text.replace(/&(#x[0-9a-f]+|#\d+|[a-z]+);/gi, (match, body: string) => {
    const lower = body.toLowerCase();
    if (keepAngleBrackets && (lower === 'lt' || lower === 'gt')) return match;
    if (lower.startsWith('#')) {
      const code =
        lower[1] === 'x'
          ? Number.parseInt(lower.slice(2), 16)
          : Number.parseInt(lower.slice(1), 10);
      return Number.isInteger(code) && code > 0 && code <= 0x10ffff
        ? String.fromCodePoint(code)
        : match;
    }
    return NAMED_ENTITIES[lower] ?? match;
  });
}

/** Display form of a source title: file names arrive percent-encoded, text HTML-escaped. */
export function cleanTitle(title: string): string {
  let decoded = title;
  try {
    decoded = decodeURIComponent(title);
  } catch {
    // Not percent-encoded (or a stray `%`): keep the raw title.
  }
  return decodeEntities(decoded);
}

/** The last path segment of a folder URI, decoded, for labelling a scope. */
export function folderName(uri: string): string {
  const segment = uri.split('/').filter(Boolean).at(-1) ?? uri;
  return cleanTitle(segment);
}

/** Whether a folder lies within an enrolled root, mirroring the backend scope authorization. */
export function folderIsEnrolled(
  folder: { readonly providerId: string; readonly uri: string },
  roots: readonly {
    readonly location: { readonly providerId: string; readonly uri: string };
    readonly recursive: boolean;
  }[],
): boolean {
  return roots.some((root) => {
    if (root.location.providerId !== folder.providerId) return false;
    const base = root.location.uri.replace(/\/+$/u, '');
    const candidate = folder.uri.replace(/\/+$/u, '');
    return candidate === base || (root.recursive && candidate.startsWith(`${base}/`));
  });
}

/** A line that starts its own Markdown block, so it must keep its line break. */
/** A bullet with no text: the Markdown renderer would leak its list placeholder. */
const EMPTY_MARKER = /^[-*+•–]$/;
const BLOCK_START = /^(?:[-*+] |\d+[.)] |#{1,6} |> |\||```)/;

/**
 * Re-flows hard-wrapped PDF text: a single line break inside a block is a wrap, not a
 * paragraph, so it becomes a space (a hyphenated break is rejoined). Blank lines stay
 * paragraph breaks, and list/heading/table lines keep their own line.
 */
export function reflowPassage(text: string): string {
  return text
    .split(/\n[ \t]*\n/)
    .map((block) => {
      const lines = block
        .split('\n')
        .map((line) => line.trim())
        .filter((line) => line.length > 0 && !EMPTY_MARKER.test(line));
      let out = '';
      for (const line of lines) {
        if (out.length === 0) out = line;
        else if (BLOCK_START.test(line) || BLOCK_START.test(out.slice(out.lastIndexOf('\n') + 1)))
          out += `\n${line}`;
        else if (/[\p{L}]-$/u.test(out) && /^\p{Ll}/u.test(line)) out += line;
        else out += ` ${line}`;
      }
      return out;
    })
    .filter((block) => block.length > 0)
    .join('\n\n');
}
