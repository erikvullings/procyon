import m, { type FactoryComponent, type Vnode } from 'mithril';
import { Skeleton } from 'mithril-materialized';

import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  CorpusThemeStatus,
  CorpusThemes,
  CorpusThemesResponse,
  RagScope,
  ThemeMapPoint,
  ThemePassage,
} from '../../models';
import { safeMarkdownHtml } from '../editor/markdown-preview';
import { knowledgeProvenanceLabel } from './knowledge-search-dialog';
import {
  cleanTitle,
  decodeEntities,
  folderIsEnrolled,
  folderName,
  isThemesStale,
  NO_THEME_COLOUR,
  percentText,
  pickPoint,
  resolveColour,
  type ScatterGeometry,
  themeColour,
  themeKeywordLabel,
  themesAreStale,
  topDocumentsForTheme,
  toScreen,
  usableKeywords,
} from './themes-model';

export interface ThemesViewAttrs {
  readonly client: FileManagerClient;
  /** Scopes offered to analyse, widest first; the backend re-authorizes each request. */
  readonly scopes: readonly RagScope[];
  /** Opens the source of a passage, at its line when the provenance has one. A click on a
   * map point calls this directly; hovering only shows the passage. */
  readonly onOpenPassage: (passage: ThemePassage) => void | Promise<void>;
  /** Starts a search limited to the given authorized sources, about the given subject. */
  readonly onSearchSources: (sourceIds: readonly string[], subject: string) => void;
  /** Smallest share of a document's passages for it to be listed under a theme (default 5%). */
  readonly minDocumentShare?: number;
  readonly onClose: () => void;
}

/** Selection in the list or on the map; `null` is the "no theme" bucket. */
type Selection = number | null | undefined;

const STATUS_POLL_MS = 15_000;
const MAP_HEIGHT = 420;
const MAP_PADDING = 14;
const HIT_RADIUS = 9;
/** Documents listed for a theme: at most this many, each with at least `minDocumentShare`. */
const TOP_DOCUMENTS = 10;
const DEFAULT_MIN_DOCUMENT_SHARE = 0.05;

function errorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message.length > 0) return error.message;
  if (
    typeof error === 'object' &&
    error !== null &&
    'message' in error &&
    typeof error.message === 'string' &&
    error.message.length > 0
  ) {
    return error.message;
  }
  return fallback;
}

interface ScatterAttrs {
  readonly points: readonly ThemeMapPoint[];
  readonly selected: Selection;
  readonly hovered: ThemeMapPoint | undefined;
  readonly onHover: (point: ThemeMapPoint | undefined) => void;
  readonly onPick: (point: ThemeMapPoint) => void;
}

/** Canvas scatter: thousands of points without one DOM node per point. */
const Scatter: FactoryComponent<ScatterAttrs> = () => {
  let canvas: HTMLCanvasElement | undefined;

  function geometry(element: HTMLCanvasElement): ScatterGeometry {
    return { width: element.clientWidth, height: MAP_HEIGHT, padding: MAP_PADDING };
  }

  function draw(attrs: ScatterAttrs): void {
    if (canvas === undefined) return;
    const width = canvas.clientWidth;
    if (width === 0) return;
    const ratio = window.devicePixelRatio || 1;
    canvas.width = Math.round(width * ratio);
    canvas.height = Math.round(MAP_HEIGHT * ratio);
    const context = canvas.getContext('2d');
    if (context === null) return;
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, width, MAP_HEIGHT);
    const frame = geometry(canvas);
    const drawPoint = (point: ThemeMapPoint, emphasised: boolean): void => {
      const { px, py } = toScreen(point, frame);
      context.beginPath();
      context.arc(px, py, emphasised ? 4.5 : 3, 0, Math.PI * 2);
      context.fill();
    };
    const colours = new Map<string, string>();
    const colourOf = (theme: number | null | undefined): string => {
      const key = themeColour(theme);
      let resolved = colours.get(key);
      if (resolved === undefined) {
        resolved = resolveColour(key, canvas as HTMLCanvasElement);
        colours.set(key, resolved);
      }
      return resolved;
    };
    const matches = (point: ThemeMapPoint): boolean =>
      attrs.selected === undefined || (point.theme ?? null) === attrs.selected;
    context.globalAlpha = attrs.selected === undefined ? 0.85 : 0.18;
    for (const point of attrs.points) {
      if (matches(point)) continue;
      context.fillStyle = colourOf(point.theme);
      drawPoint(point, false);
    }
    context.globalAlpha = 0.95;
    for (const point of attrs.points) {
      if (!matches(point)) continue;
      context.fillStyle = colourOf(point.theme);
      drawPoint(point, attrs.selected !== undefined);
    }
    if (attrs.hovered !== undefined) {
      const { px, py } = toScreen(attrs.hovered, frame);
      context.globalAlpha = 1;
      context.lineWidth = 2;
      context.strokeStyle = getComputedStyle(canvas).color || 'CanvasText';
      context.beginPath();
      context.arc(px, py, 7, 0, Math.PI * 2);
      context.stroke();
    }
  }

  function locate(
    event: PointerEvent,
    attrs: ScatterAttrs,
  ): { readonly point: ThemeMapPoint | undefined } {
    if (canvas === undefined) return { point: undefined };
    const rect = canvas.getBoundingClientRect();
    return {
      point: pickPoint(
        attrs.points,
        event.clientX - rect.left,
        event.clientY - rect.top,
        geometry(canvas),
        HIT_RADIUS,
      ),
    };
  }

  return {
    oncreate: ({ dom, attrs }) => {
      canvas = dom as HTMLCanvasElement;
      draw(attrs);
    },
    onupdate: ({ attrs }) => draw(attrs),
    view: ({ attrs }) =>
      m('canvas.fm-themes-map-canvas', {
        // The theme list is the accessible alternative; the canvas is decorative.
        'aria-hidden': 'true',
        style: { height: `${MAP_HEIGHT}px` },
        onpointermove: (event: PointerEvent) => {
          const { point } = locate(event, attrs);
          if (point !== attrs.hovered) attrs.onHover(point);
        },
        onpointerleave: () => attrs.onHover(undefined),
        onclick: (event: PointerEvent) => {
          const { point } = locate(event, attrs);
          if (point !== undefined) attrs.onPick(point);
        },
      }),
  };
};

/** Readable tail of a SKOS concept identifier (`…/concept/wind-energy` -> `wind energy`). */
function conceptLabel(conceptId: string): string {
  const tail = conceptId.split(/[/#]/).filter(Boolean).at(-1) ?? conceptId;
  let decoded = tail;
  try {
    decoded = decodeURIComponent(tail);
  } catch {
    // Keep the raw segment when it is not URI encoded.
  }
  return decoded.replace(/[_-]+/g, ' ').trim() || conceptId;
}

/** A document row opened like a passage: the host only needs its source and title. */
function documentAsPassage(document: {
  readonly sourceId: string;
  readonly title: string;
}): ThemePassage {
  return {
    chunkId: '',
    excerpt: '',
    provenance: '',
    sectionPath: [],
    sourceId: document.sourceId,
    sourcePosition: 0,
    title: document.title,
  };
}

/** Passages longer than this get a show more / show less toggle. */
const COLLAPSED_PASSAGE_CHARS = 260;

interface PassageTextAttrs {
  readonly excerpt: string;
}

/** A passage rendered as Markdown, clamped to a few lines until expanded. */
const PassageText: FactoryComponent<PassageTextAttrs> = () => {
  let expanded = false;
  return {
    view: ({ attrs }) => {
      const long = attrs.excerpt.length > COLLAPSED_PASSAGE_CHARS;
      return m('.fm-themes-passage-body', [
        m('.fm-themes-excerpt', {
          class: long && !expanded ? 'is-clamped' : '',
          // Sanitized by `safeMarkdownHtml` (DOMPurify), like the Markdown file preview.
          innerHTML: safeMarkdownHtml(decodeEntities(attrs.excerpt, true)),
        }),
        long
          ? m(
              'button.btn-flat.fm-themes-expand',
              {
                type: 'button',
                'aria-expanded': String(expanded),
                onclick: () => {
                  expanded = !expanded;
                },
              },
              t('themes', expanded ? 'showLess' : 'showMore'),
            )
          : undefined,
      ]);
    },
  };
};

function passageRow(
  passage: ThemePassage,
  attrs: ThemesViewAttrs,
  failed: (message: string) => void,
): Vnode {
  const location = knowledgeProvenanceLabel(passage.provenance);
  const title = passage.title.length > 0 ? cleanTitle(passage.title) : t('themes', 'untitled');
  return m('li.fm-themes-passage', { key: passage.chunkId }, [
    m('.fm-themes-passage-head', [
      m(
        'a.fm-themes-source',
        {
          href: '#',
          title: t('themes', 'openSource'),
          onclick: (event: MouseEvent) => {
            event.preventDefault();
            void Promise.resolve(attrs.onOpenPassage(passage)).catch((cause: unknown) =>
              failed(errorMessage(cause, t('themes', 'openSourceFailed'))),
            );
          },
        },
        title,
      ),
      location.length > 0 ? m('span.fm-themes-muted', ` · ${location}`) : undefined,
    ]),
    m(PassageText, { excerpt: passage.excerpt }),
  ]);
}

/** Corpus themes as a full pane view (map, theme list, details), like disk usage. */
export const ThemesView: FactoryComponent<ThemesViewAttrs> = () => {
  let busy = false;
  let error: string | undefined;
  let response: CorpusThemesResponse | undefined;
  let status: CorpusThemeStatus | undefined;
  let selected: Selection;
  let selectedPoint: ThemeMapPoint | undefined;
  let hovered: ThemeMapPoint | undefined;
  let poll: ReturnType<typeof setInterval> | undefined;
  let generation = 0;
  let scopeIndex = 0;
  let visibleScopes: readonly RagScope[] = [];

  function activeScope(): RagScope | undefined {
    return visibleScopes[Math.min(scopeIndex, visibleScopes.length - 1)];
  }

  /** Offers the folder scope only when the backend would authorize it (an enrolled root). */
  async function resolveScopes(attrs: ThemesViewAttrs): Promise<readonly RagScope[]> {
    const folderScopes = attrs.scopes.filter((scope) => scope.kind === 'currentFolder');
    if (folderScopes.length === 0) return attrs.scopes;
    let roots: Awaited<ReturnType<FileManagerClient['listKnowledgeRoots']>> = [];
    try {
      roots = await attrs.client.listKnowledgeRoots({
        workspaceId: folderScopes[0]?.workspaceId ?? '',
      });
    } catch {
      // Without the roots the folder scope cannot be vouched for: offer the library only.
    }
    return attrs.scopes.filter(
      (scope) =>
        scope.kind !== 'currentFolder' ||
        (scope.folder != null && folderIsEnrolled(scope.folder, roots)),
    );
  }

  function stopPolling(): void {
    if (poll !== undefined) clearInterval(poll);
    poll = undefined;
  }

  async function refreshStatus(attrs: ThemesViewAttrs, ticket: number): Promise<void> {
    const scope = activeScope();
    if (scope === undefined) return;
    try {
      const next = await attrs.client.getCorpusThemeStatus({ scope });
      if (ticket === generation) status = next;
    } catch {
      // The indicator is advisory; the next poll retries.
    }
    m.redraw();
  }

  async function load(attrs: ThemesViewAttrs): Promise<void> {
    const scope = activeScope();
    if (scope === undefined) return;
    generation += 1;
    const ticket = generation;
    busy = true;
    error = undefined;
    m.redraw();
    selected = undefined;
    selectedPoint = undefined;
    hovered = undefined;
    try {
      const result = await attrs.client.analyzeCorpusThemes({ scope });
      if (ticket !== generation) return;
      response = result;
      status = undefined;
      await refreshStatus(attrs, ticket);
    } catch (cause) {
      if (ticket !== generation) return;
      response = undefined;
      error = errorMessage(cause, t('themes', 'loadFailed'));
    } finally {
      if (ticket === generation) busy = false;
      m.redraw();
    }
  }

  function selectTheme(theme: Selection): void {
    selected = selected === theme ? undefined : theme;
    selectedPoint = undefined;
  }

  async function searchWithinTheme(
    attrs: ThemesViewAttrs,
    themes: CorpusThemes,
    theme: number | null,
  ): Promise<void> {
    const scope = activeScope();
    if (scope === undefined) return;
    try {
      const { sourceIds } = await attrs.client.getCorpusThemeSources({
        scope,
        theme,
        fingerprint: themes.fingerprint,
      });
      error = undefined;
      // The trailing space lets the user keep typing straight after the theme words.
      attrs.onSearchSources(sourceIds, theme === null ? '' : `${themeLabel(theme, themes)} `);
    } catch (cause) {
      if (isThemesStale(cause)) {
        // The index moved on: show current themes instead of searching a stale set.
        void load(attrs);
        return;
      }
      error = errorMessage(cause, t('themes', 'searchFailed'));
    }
    m.redraw();
  }

  function themeDetails(themes: CorpusThemes, attrs: ThemesViewAttrs): Vnode | undefined {
    if (selected === undefined) return undefined;
    const theme =
      selected === null ? undefined : themes.themes.find((item) => item.id === selected);
    const passages = selected === null ? themes.noTheme.passages : (theme?.passages ?? []);
    const minShare = attrs.minDocumentShare ?? DEFAULT_MIN_DOCUMENT_SHARE;
    const documents = topDocumentsForTheme(
      themes.documentMixtures,
      selected,
      TOP_DOCUMENTS,
      minShare,
    );
    const fail = (message: string): void => {
      error = message;
      m.redraw();
    };
    const picked = selectedPoint;
    return m('section.fm-themes-details', { 'aria-live': 'polite' }, [
      m('h4', selected === null ? t('themes', 'noTheme') : themeLabel(selected, themes)),
      m(
        'button.btn.fm-themes-search',
        {
          type: 'button',
          disabled: busy,
          onclick: () => void searchWithinTheme(attrs, themes, selected ?? null),
        },
        t('themes', 'searchInTheme'),
      ),
      theme === undefined
        ? m('p.fm-themes-muted', t('themes', 'noThemeHint'))
        : m('.fm-themes-keywords', [
            m('h5', t('themes', 'keywords')),
            m(
              'ul.fm-themes-chips',
              usableKeywords(theme.keywords).map((keyword) =>
                m('li', { key: keyword.term }, keyword.term),
              ),
            ),
          ]),
      documents.length === 0
        ? undefined
        : m('.fm-themes-documents', [
            m('h5', t('themes', 'topDocuments')),
            m(
              'ol.fm-themes-document-list',
              documents.map((document) =>
                m('li', { key: document.sourceId }, [
                  m(
                    'a.fm-themes-source',
                    {
                      href: '#',
                      title: t('themes', 'openSource'),
                      onclick: (event: MouseEvent) => {
                        event.preventDefault();
                        void Promise.resolve(
                          attrs.onOpenPassage(documentAsPassage(document)),
                        ).catch((cause: unknown) =>
                          fail(errorMessage(cause, t('themes', 'openSourceFailed'))),
                        );
                      },
                    },
                    document.title.length > 0
                      ? cleanTitle(document.title)
                      : t('themes', 'untitled'),
                  ),
                  m('span.fm-themes-muted', ` · ${percentText(document.share)}`),
                ]),
              ),
            ),
          ]),
      theme === undefined || theme.concepts.length === 0
        ? undefined
        : m('.fm-themes-concepts', [
            m('h5', t('themes', 'concepts')),
            m(
              'ul.fm-themes-chips',
              theme.concepts.map((concept) =>
                m(
                  'li',
                  { key: concept.conceptId, title: concept.conceptId },
                  `${conceptLabel(concept.conceptId)} (${concept.chunkCount})`,
                ),
              ),
            ),
          ]),
      picked === undefined
        ? undefined
        : m('.fm-themes-selected-point', [
            m('h5', t('themes', 'selectedPassage')),
            m('ul.fm-themes-passages', [
              passageRow(
                passages.find((item) => item.chunkId === picked.passage.chunkId) ?? picked.passage,
                attrs,
                fail,
              ),
            ]),
          ]),
      passages.length === 0
        ? undefined
        : m('.fm-themes-examples', [
            m('h5', t('themes', 'examples')),
            m('p.fm-themes-muted', t('themes', 'examplesHint')),
            m(
              'ul.fm-themes-passages',
              passages.map((passage) => passageRow(passage, attrs, fail)),
            ),
          ]),
    ]);
  }

  function themeLabel(id: number, themes: CorpusThemes): string {
    const theme = themes.themes.find((item) => item.id === id);
    const keywords = theme === undefined ? '' : themeKeywordLabel(theme.keywords);
    return keywords.length > 0 ? keywords : t('themes', 'themeNumber', { number: id + 1 });
  }

  function themeList(themes: CorpusThemes): Vnode {
    const row = (key: number | null, label: string, chunkCount: number, share: number): Vnode =>
      m(
        'li',
        { key: key ?? 'none' },
        m(
          'button.fm-themes-row',
          {
            type: 'button',
            'aria-pressed': String(selected === key),
            title: t('themes', 'selectTheme'),
            onclick: () => selectTheme(key),
          },
          [
            m('span.fm-themes-swatch', {
              style: { background: key === null ? NO_THEME_COLOUR : themeColour(key) },
              'aria-hidden': 'true',
            }),
            m('span.fm-themes-row-label', label),
            m('span.fm-themes-muted', `${chunkCount} · ${percentText(share)}`),
          ],
        ),
      );
    return m('ul.fm-themes-list', { 'aria-label': t('themes', 'listHeading') }, [
      ...themes.themes.map((theme) =>
        row(theme.id, themeLabel(theme.id, themes), theme.chunkCount, theme.share),
      ),
      themes.noTheme.chunkCount > 0
        ? row(null, t('themes', 'noTheme'), themes.noTheme.chunkCount, themes.noTheme.share)
        : undefined,
    ]);
  }

  function outcomeMessage(result: CorpusThemesResponse): string | undefined {
    switch (result.outcome) {
      case 'ready':
        return undefined;
      case 'profileTooWeak':
        return t('themes', 'outcomeProfileTooWeak', { model: result.modelId });
      case 'insufficientContent':
        return t('themes', 'outcomeInsufficient');
      case 'noContent':
        return t('themes', 'outcomeNoContent');
    }
  }

  function start(attrs: ThemesViewAttrs): void {
    scopeIndex = 0;
    busy = true;
    visibleScopes = attrs.scopes.filter((scope) => scope.kind !== 'currentFolder');
    void resolveScopes(attrs).then((resolved) => {
      visibleScopes = resolved;
      void load(attrs);
    });
    stopPolling();
    poll = setInterval(() => {
      if (!busy) void refreshStatus(attrs, generation);
    }, STATUS_POLL_MS);
  }

  return {
    oncreate: ({ attrs }) => start(attrs),
    onremove: () => {
      generation += 1;
      stopPolling();
    },
    view: ({ attrs }) => {
      const themes = response?.themes ?? undefined;
      const message = response === undefined ? undefined : outcomeMessage(response);
      const stale = themes !== undefined && themesAreStale(themes, status);
      return m('.fm-themes.fm-themes-view', [
        m('.fm-themes-header', [
          m('h3', t('themes', 'title')),
          m('button.btn-flat', { type: 'button', onclick: attrs.onClose }, t('button', 'close')),
        ]),
        m('.fm-themes-content', [
          visibleScopes.length > 1
            ? m(
                '.fm-themes-scopes',
                { role: 'group', 'aria-label': t('themes', 'scopeLabel') },
                visibleScopes.map((scope, index) =>
                  m(
                    'button.fm-themes-scope',
                    {
                      type: 'button',
                      key: scope.kind,
                      'aria-pressed': String(index === scopeIndex),
                      disabled: busy,
                      onclick: () => {
                        if (index === scopeIndex) return;
                        scopeIndex = index;
                        void load(attrs);
                      },
                    },
                    scope.kind === 'currentFolder' && scope.folder != null
                      ? `${scope.label}: ${folderName(scope.folder.uri)}`
                      : scope.label,
                  ),
                ),
              )
            : undefined,
          busy
            ? m('.fm-themes-progress', { role: 'status', 'aria-busy': 'true' }, [
                m('span', t('themes', 'analyzing')),
                m(Skeleton, { count: 4, className: 'fm-themes-skeleton' }),
              ])
            : undefined,
          error === undefined ? undefined : m('p.fm-themes-error', { role: 'alert' }, error),
          message === undefined ? undefined : m('p.fm-themes-notice', { role: 'status' }, message),
          stale
            ? m('.fm-themes-notice.fm-themes-stale', { role: 'status' }, [
                m('span', t('themes', 'stale')),
                m(
                  'button.btn-flat',
                  { type: 'button', disabled: busy, onclick: () => void load(attrs) },
                  t('themes', 'refresh'),
                ),
              ])
            : undefined,
          themes === undefined
            ? undefined
            : m('.fm-themes-body', [
                m(
                  'p.fm-themes-muted',
                  t('themes', 'analysed', {
                    documents: themes.documentCount,
                    passages: themes.chunkCount,
                  }),
                ),
                m('.fm-themes-layout', [
                  m('.fm-themes-map', [
                    m('h4', t('themes', 'mapHeading')),
                    m(Scatter, {
                      points: themes.mapPoints,
                      selected,
                      hovered,
                      onHover: (point) => {
                        hovered = point;
                      },
                      onPick: (point) => {
                        selected = point.theme ?? null;
                        selectedPoint = point;
                        void Promise.resolve(attrs.onOpenPassage(point.passage)).catch(
                          (cause: unknown) => {
                            error = errorMessage(cause, t('themes', 'openSourceFailed'));
                            m.redraw();
                          },
                        );
                      },
                    }),
                    m('p.fm-themes-muted', t('themes', 'mapLabel')),
                    m('p.fm-themes-muted', t('themes', 'mapHint')),
                    themes.mapSampled
                      ? m(
                          'p.fm-themes-muted',
                          t('themes', 'mapSampled', {
                            shown: themes.mapPoints.length,
                            total: themes.chunkCount,
                          }),
                        )
                      : undefined,
                  ]),
                  m('.fm-themes-listing', [m('h4', t('themes', 'listHeading')), themeList(themes)]),
                ]),
                m(
                  'p.fm-themes-hover',
                  { role: 'status' },
                  hovered === undefined
                    ? undefined
                    : [
                        m('strong', cleanTitle(hovered.passage.title)),
                        ` — ${decodeEntities(hovered.passage.excerpt)}`,
                      ],
                ),
                themeDetails(themes, attrs),
              ]),
        ]),
      ]);
    },
  };
};
