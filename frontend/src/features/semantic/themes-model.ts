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

/** Label of a theme: its leading keywords, or a numbered fallback supplied by the caller. */
export function themeKeywordLabel(
  keywords: readonly { readonly term: string }[],
  limit = 3,
): string {
  return keywords
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
): readonly ThemeDocumentShare[] {
  const rows: ThemeDocumentShare[] = [];
  for (const mixture of mixtures) {
    const slice = mixture.shares.find((share) => (share.theme ?? null) === theme);
    if (slice !== undefined) {
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
