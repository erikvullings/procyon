import m, { type FactoryComponent, type Vnode, type VnodeDOM } from 'mithril';
import {
  Button,
  FlatButton,
  IconButton,
  InputCheckbox,
  ModalPanel,
  RadioButtons,
  Select,
  Skeleton,
  TextInput,
} from 'mithril-materialized';

import type { SemanticClient } from '../../api/client/semantic-client.ts';
import {
  chevronDownIcon,
  chevronUpIcon,
  downloadIcon,
  folderOpenIcon,
} from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type {
  CorpusThemeExportFormat,
  CorpusThemeStatus,
  CorpusThemes,
  CorpusThemesResponse,
  KnowledgeRoot,
  Location,
  RagScope,
  ThemeMapPoint,
  ThemePassage,
} from '../../models';
import { safeMarkdownHtml } from '../editor/markdown-preview';
import { newTabButton } from '../panes/tab-strip';
import { knowledgeProvenanceLabel } from './knowledge-evidence-view';
import {
  cleanTitle,
  decodeEntities,
  exportFileName,
  folderIsEnrolled,
  folderName,
  isThemesStale,
  NO_THEME_COLOUR,
  percentText,
  pickPoint,
  reflowPassage,
  resolveColour,
  type ScatterGeometry,
  scopeForRoots,
  themeColour,
  themeExportLabels,
  themeKeywordLabel,
  themesAreStale,
  topDocumentsForTheme,
  toScreen,
  usableKeywords,
  validRootSelection,
} from './themes-model';

export interface ThemesViewAttrs {
  readonly client: SemanticClient;
  /** Scopes offered to analyse, widest first; the backend re-authorizes each request. */
  readonly scopes: readonly RagScope[];
  /** Opens the source of a passage, at its line when the provenance has one. A click on a
   * map point calls this directly; hovering only shows the passage. */
  readonly onOpenPassage: (passage: ThemePassage) => void | Promise<void>;
  /** Starts a search limited to the given authorized sources, about the given subject. */
  readonly onSearchSources: (sourceIds: readonly string[], subject: string) => void;
  /** Smallest share of a document's passages for it to be listed under a theme (default 5%). */
  readonly minDocumentShare?: number;
  /** Remembered indexed roots to analyse (global setting); empty or absent means all of them. */
  readonly selectedRootIds?: readonly string[];
  /** Remembers the chosen roots; called with `[]` when every root is chosen. */
  readonly onSelectRoots?: (rootIds: readonly string[]) => void;
  /** Folder an export is saved in (the active pane's folder). Without it, export is not offered. */
  readonly exportFolder?: Location;
  /** Told after a successful export, with the saved file's name. */
  readonly onExported?: (fileName: string) => void;
  /** Opens a new tab beside this one; the breadcrumb bar offers the button when given. */
  readonly onNewTab?: () => void;
}

const EXPORT_FORMATS: readonly {
  readonly format: CorpusThemeExportFormat;
  readonly label:
    | 'exportFormatMarkdown'
    | 'exportFormatDocx'
    | 'exportFormatSkosTurtle'
    | 'exportFormatSkosRdfXml'
    | 'exportFormatSkosJsonLd';
}[] = [
  { format: 'markdown', label: 'exportFormatMarkdown' },
  { format: 'docx', label: 'exportFormatDocx' },
  { format: 'skosTurtle', label: 'exportFormatSkosTurtle' },
  { format: 'skosRdfXml', label: 'exportFormatSkosRdfXml' },
  { format: 'skosJsonLd', label: 'exportFormatSkosJsonLd' },
];

/** The views the status bar toggles between; the map is the reason the pane was opened. */
const isSkos = (format: CorpusThemeExportFormat): boolean => format.startsWith('skos');

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

interface PassageAttrs {
  readonly passage: ThemePassage;
  readonly attrs: ThemesViewAttrs;
  readonly failed: (message: string) => void;
}

/**
 * One passage: the source link and an expand toggle share the head line, and the text is
 * clamped to a few lines. The toggle only appears when the clamp actually hides text.
 */
const PassageItem: FactoryComponent<PassageAttrs> = () => {
  let expanded = false;
  let overflowing = false;
  let excerpt: HTMLElement | undefined;
  const measure = (): void => {
    if (excerpt === undefined || expanded) return;
    const next = excerpt.scrollHeight > excerpt.clientHeight + 1;
    if (next !== overflowing) {
      overflowing = next;
      m.redraw();
    }
  };
  return {
    oncreate: measure,
    onupdate: measure,
    view: ({ attrs: { passage, attrs, failed } }) => {
      const location = knowledgeProvenanceLabel(passage.provenance);
      const title = passage.title.length > 0 ? cleanTitle(passage.title) : t('themes', 'untitled');
      const toggleLabel = t('themes', expanded ? 'showLess' : 'showMore');
      return m('.fm-themes-passage-item', [
        m('.fm-themes-passage-head', [
          m('.fm-themes-passage-title', [
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
          overflowing || expanded
            ? m(
                'button.fm-themes-expand',
                {
                  type: 'button',
                  title: toggleLabel,
                  'aria-label': toggleLabel,
                  'aria-expanded': String(expanded),
                  onclick: () => {
                    expanded = !expanded;
                  },
                },
                [
                  m('span', toggleLabel),
                  expanded ? chevronUpIcon({ size: 14 }) : chevronDownIcon({ size: 14 }),
                ],
              )
            : undefined,
        ]),
        m('.fm-themes-excerpt', {
          class: expanded ? '' : 'is-clamped',
          oncreate: (vnode: VnodeDOM) => {
            excerpt = vnode.dom as HTMLElement;
          },
          // Sanitized by `safeMarkdownHtml` (DOMPurify), like the Markdown file preview.
          innerHTML: safeMarkdownHtml(reflowPassage(decodeEntities(passage.excerpt, true))),
        }),
      ]);
    },
  };
};

function passageRow(
  passage: ThemePassage,
  attrs: ThemesViewAttrs,
  failed: (message: string) => void,
): Vnode {
  return m(
    'li.fm-themes-passage',
    { key: passage.chunkId },
    m(PassageItem, { passage, attrs, failed }),
  );
}

const ScopeRadios = RadioButtons<number>();
const ExportScopeRadios = RadioButtons<'all' | 'selected'>();
const FormatSelect = Select<CorpusThemeExportFormat>();

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
  let roots: readonly KnowledgeRoot[] = [];
  /** Roots the shown themes were computed for, and the checkboxes' not yet applied state. */
  let appliedRoots: readonly string[] = [];
  let draftRoots: readonly string[] = [];
  let dialog: 'folders' | 'export' | undefined;
  let exportFormat: CorpusThemeExportFormat = 'markdown';
  let exportOnlySelected = false;
  let includePassages = false;
  let includeDocuments = false;
  let baseUri = '';
  let exporting = false;
  let exportResult: { readonly ok: boolean; readonly text: string } | undefined;

  function baseScope(): RagScope | undefined {
    return visibleScopes[Math.min(scopeIndex, visibleScopes.length - 1)];
  }

  /** A scope's name, with the folder it is about when there is one. */
  function scopeText(scope: RagScope): string {
    return scope.kind === 'currentFolder' && scope.folder != null
      ? `${scope.label}: ${folderName(scope.folder.uri)}`
      : scope.label;
  }

  /** The library scope narrows to the chosen roots; other scopes are used as offered. */
  function activeScope(): RagScope | undefined {
    const base = baseScope();
    if (base === undefined || base.kind !== 'entireLibrary') return base;
    return scopeForRoots(base, appliedRoots, roots, t('themes', 'scopeSelectedFolders'));
  }

  /** The indexed roots, without which a folder scope cannot be vouched for. */
  async function loadRoots(attrs: ThemesViewAttrs): Promise<readonly KnowledgeRoot[]> {
    const workspaceId = attrs.scopes[0]?.workspaceId ?? '';
    try {
      return await attrs.client.listKnowledgeRoots({ workspaceId });
    } catch {
      return [];
    }
  }

  /** Offers the folder scope only when the backend would authorize it (an enrolled root). */
  function resolveScopes(attrs: ThemesViewAttrs): readonly RagScope[] {
    return attrs.scopes.filter(
      (scope) =>
        scope.kind !== 'currentFolder' ||
        (scope.folder != null && folderIsEnrolled(scope.folder, roots)),
    );
  }

  function rootLabel(root: KnowledgeRoot): string {
    return root.label.length > 0 ? root.label : folderName(root.location.uri);
  }

  /** Folder name and the documents indexed in it. */
  function rootOption(root: KnowledgeRoot): string {
    const count = root.indexedSources;
    return `${rootLabel(root)} · ${t('themes', 'folderDocuments', { count })}`;
  }

  function toggleRoot(rootId: string, checked: boolean): void {
    draftRoots = checked
      ? [...draftRoots.filter((id) => id !== rootId), rootId]
      : draftRoots.filter((id) => id !== rootId);
  }

  function rootsChanged(): boolean {
    const sorted = (ids: readonly string[]): string => [...ids].sort().join('\n');
    return sorted(draftRoots) !== sorted(appliedRoots);
  }

  function applyRoots(attrs: ThemesViewAttrs): void {
    const chosen = roots.map((root) => root.rootId).filter((id) => draftRoots.includes(id));
    if (chosen.length === 0) return;
    appliedRoots = chosen;
    draftRoots = chosen;
    attrs.onSelectRoots?.(chosen.length === roots.length ? [] : chosen);
    dialog = undefined;
    void load(attrs);
  }

  /** Folder multi-select: unrelated topics cluster poorly together, so analyse a chosen subset. */
  function hasRootsPanel(): boolean {
    return roots.length >= 2 && baseScope()?.kind === 'entireLibrary';
  }

  /** Folder dialog: the scope to analyse, then the folders a library scope narrows to. */
  function foldersBody(attrs: ThemesViewAttrs): Vnode {
    return m('.fm-themes-dialog-body.fm-themes-folders', [
      visibleScopes.length > 1
        ? m(ScopeRadios, {
            label: t('themes', 'scopeLabel'),
            options: visibleScopes.map((scope, index) => ({ id: index, label: scopeText(scope) })),
            checkedId: scopeIndex,
            disabled: busy,
            onchange: (index: number) => {
              if (index === scopeIndex) return;
              scopeIndex = index;
              void load(attrs);
            },
          })
        : undefined,
      hasRootsPanel()
        ? m('section.fm-themes-roots', [
            m('p.fm-themes-muted', t('themes', 'foldersHint')),
            m(
              'ul.fm-themes-root-list',
              roots.map((root) =>
                m(
                  'li',
                  { key: root.rootId },
                  m(InputCheckbox, {
                    inputId: `themes-root-${root.rootId}`,
                    className: 'fm-themes-checkbox',
                    label: rootOption(root),
                    checked: draftRoots.includes(root.rootId),
                    disabled: busy,
                    onchange: (checked: boolean) => toggleRoot(root.rootId, checked),
                  }),
                ),
              ),
            ),
            m('.fm-themes-root-actions', [
              m(Button, {
                label: t('themes', 'applyFolders'),
                className: 'fm-themes-action',
                disabled: busy || draftRoots.length === 0 || !rootsChanged(),
                onclick: () => applyRoots(attrs),
              }),
              m(FlatButton, {
                label: t('themes', 'selectAllFolders'),
                disabled: busy || draftRoots.length === roots.length,
                onclick: () => {
                  draftRoots = roots.map((root) => root.rootId);
                },
              }),
            ]),
          ])
        : undefined,
    ]);
  }

  async function runExport(attrs: ThemesViewAttrs, themes: CorpusThemes): Promise<void> {
    const scope = activeScope();
    const folder = attrs.exportFolder;
    if (scope === undefined || folder === undefined || exporting) return;
    const onlyTheme = exportOnlySelected && typeof selected === 'number' ? selected : undefined;
    const fileName = exportFileName(
      exportFormat,
      new Date(),
      onlyTheme === undefined ? undefined : onlyTheme + 1,
    );
    // Join onto the pane's directory URI: the file lands where the user is browsing and both hosts
    // write it through the same provider path.
    const base = folder.uri.endsWith('/') ? folder.uri.slice(0, -1) : folder.uri;
    exporting = true;
    exportResult = undefined;
    const trimmedBase = baseUri.trim();
    try {
      const saved = await attrs.client.exportCorpusThemes({
        scope,
        format: exportFormat,
        destination: {
          providerId: folder.providerId,
          uri: `${base}/${encodeURIComponent(fileName)}`,
        },
        overwrite: false,
        themes: onlyTheme === undefined ? [] : [onlyTheme],
        labels: [...themeExportLabels(themes, (number) => t('themes', 'themeNumber', { number }))],
        includePassages,
        includeDocuments,
        ...(isSkos(exportFormat) && trimmedBase.length > 0 ? { baseUri: trimmedBase } : {}),
      });
      exportResult = {
        ok: true,
        text: t('themes', 'exportDone', { name: fileName, count: saved.themeCount }),
      };
      attrs.onExported?.(fileName);
    } catch (cause) {
      exportResult = { ok: false, text: errorMessage(cause, t('themes', 'exportFailed')) };
    } finally {
      exporting = false;
      m.redraw();
    }
  }

  /** Export options. Document text and titles are opt-in; SKOS offers a base URI. */
  /** Export dialog: format, what to write, and the save action. */
  function exportBody(attrs: ThemesViewAttrs, themes: CorpusThemes): Vnode {
    const singleTheme = typeof selected === 'number';
    return m('.fm-themes-dialog-body.fm-themes-export', [
      m(FormatSelect, {
        id: 'fm-themes-export-format',
        label: t('themes', 'exportFormat'),
        options: EXPORT_FORMATS.map((entry) => ({
          id: entry.format,
          label: t('themes', entry.label),
        })),
        checkedId: exportFormat,
        onchange: (ids: CorpusThemeExportFormat[]) => {
          const [format] = ids;
          if (format !== undefined) exportFormat = format;
        },
      }),
      m(ExportScopeRadios, {
        label: t('themes', 'exportWhat'),
        options: [
          { id: 'all', label: t('themes', 'exportAll') },
          { id: 'selected', label: t('themes', 'exportSelected'), disabled: !singleTheme },
        ],
        checkedId: exportOnlySelected && singleTheme ? 'selected' : 'all',
        onchange: (id: 'all' | 'selected') => {
          exportOnlySelected = id === 'selected';
        },
      }),
      m(InputCheckbox, {
        inputId: 'fm-themes-export-passages',
        className: 'fm-themes-checkbox',
        label: t('themes', 'exportPassages'),
        checked: includePassages,
        onchange: (checked: boolean) => {
          includePassages = checked;
        },
      }),
      m(InputCheckbox, {
        inputId: 'fm-themes-export-documents',
        className: 'fm-themes-checkbox',
        label: t('themes', 'exportDocuments'),
        checked: includeDocuments,
        onchange: (checked: boolean) => {
          includeDocuments = checked;
        },
      }),
      isSkos(exportFormat)
        ? [
            m(TextInput, {
              id: 'fm-themes-export-base-uri',
              label: t('themes', 'exportBaseUri'),
              placeholder: 'https://example.org/themes/',
              value: baseUri,
              oninput: (value: string) => {
                baseUri = value;
              },
            }),
            m('p.fm-themes-muted', t('themes', 'exportBaseUriHint')),
          ]
        : undefined,
      m(Button, {
        label: exporting ? t('themes', 'exporting') : t('themes', 'exportRun'),
        className: 'fm-themes-action',
        disabled: exporting || busy,
        onclick: () => void runExport(attrs, themes),
      }),
      exportResult === undefined
        ? undefined
        : m(
            exportResult.ok ? 'p.fm-themes-notice' : 'p.fm-themes-error',
            { role: exportResult.ok ? 'status' : 'alert' },
            exportResult.text,
          ),
    ]);
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
      m('.fm-themes-details-head', [
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
      ]),
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
                  `${concept.label ?? conceptLabel(concept.conceptId)} (${concept.chunkCount})`,
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
    // Keyed rows must not be mixed with a hole: Mithril rejects a fragment that does.
    const rows = themes.themes.map((theme) =>
      row(theme.id, themeLabel(theme.id, themes), theme.chunkCount, theme.share),
    );
    if (themes.noTheme.chunkCount > 0) {
      rows.push(row(null, t('themes', 'noTheme'), themes.noTheme.chunkCount, themes.noTheme.share));
    }
    return m('ul.fm-themes-list', { 'aria-label': t('themes', 'listHeading') }, rows);
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
    void loadRoots(attrs).then((loaded) => {
      roots = loaded;
      appliedRoots = validRootSelection(attrs.selectedRootIds ?? [], roots);
      draftRoots = appliedRoots;
      visibleScopes = resolveScopes(attrs);
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
        m('.fm-breadcrumb-row.fm-themes-breadcrumb', [
          m('nav.fm-breadcrumb.fm-themes-crumbs', { 'aria-label': t('themes', 'title') }, [
            m(
              'span.fm-breadcrumb-segment.fm-themes-crumb',
              activeScope() === undefined ? undefined : scopeText(activeScope() as RagScope),
            ),
            themes === undefined
              ? undefined
              : m(
                  'span.fm-breadcrumb-segment.fm-themes-crumb',
                  t('themes', 'analysed', {
                    documents: themes.documentCount,
                    passages: themes.chunkCount,
                    shown: themes.mapPoints.length,
                  }),
                ),
          ]),
          hasRootsPanel()
            ? tooltip(
                t('themes', 'foldersTab'),
                m(
                  IconButton,
                  {
                    className: 'fm-themes-icon',
                    'aria-label': t('themes', 'foldersTab'),
                    onclick: () => {
                      dialog = 'folders';
                    },
                  },
                  folderOpenIcon({ size: 16 }),
                ),
              )
            : undefined,
          attrs.exportFolder === undefined || themes === undefined
            ? undefined
            : tooltip(
                t('themes', 'exportTitle'),
                m(
                  IconButton,
                  {
                    className: 'fm-themes-icon',
                    'aria-label': t('themes', 'exportTitle'),
                    onclick: () => {
                      dialog = 'export';
                    },
                  },
                  downloadIcon({ size: 16 }),
                ),
              ),
          attrs.onNewTab === undefined ? undefined : newTabButton(attrs.onNewTab),
        ]),
        m('.fm-themes-scroll', [
          m('.fm-themes-content', [
            busy
              ? m('.fm-themes-progress', { role: 'status', 'aria-busy': 'true' }, [
                  m('span', t('themes', 'analyzing')),
                  m(Skeleton, { count: 4, className: 'fm-themes-skeleton' }),
                ])
              : undefined,
            error === undefined ? undefined : m('p.fm-themes-error', { role: 'alert' }, error),
            message === undefined
              ? undefined
              : m('p.fm-themes-notice', { role: 'status' }, message),
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
                    ]),
                    m('.fm-themes-listing', [
                      m('h4', t('themes', 'listHeading')),
                      themeList(themes),
                    ]),
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
        ]),
        dialog === undefined
          ? undefined
          : m(ModalPanel, {
              className: 'fm-dense-modal fm-themes-dialog',
              title:
                dialog === 'folders' ? t('themes', 'foldersHeading') : t('themes', 'exportTitle'),
              isOpen: true,
              closeOnEsc: true,
              onToggle: (open: boolean) => {
                if (!open) dialog = undefined;
              },
              description:
                dialog === 'folders'
                  ? foldersBody(attrs)
                  : themes === undefined
                    ? undefined
                    : exportBody(attrs, themes),
            }),
      ]);
    },
  };
};
