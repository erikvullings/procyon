import m, { type FactoryComponent } from 'mithril';
import { FlatButton, ModalPanel, PaginationControls, Select, toast } from 'mithril-materialized';
import {
  closeIcon,
  copyIcon,
  infoCircleIcon,
  menuIcon,
  searchIcon,
} from '../../components/tabler-icons';
import { tooltip } from '../../components/tooltip';
import { t } from '../../i18n';
import type { GitLogEntry } from '../../models';
import { calculateVisibleWindow } from '../directory-table/windowing';
import { CodeMirrorEditor } from '../editor/code-mirror-editor';
import { editableLanguageForExtension, languageExtension } from '../editor/editor-language';
import { safeMarkdownHtml } from '../editor/markdown-preview';
import {
  DEFAULT_ENTRY_FORMAT_SETTINGS,
  formatEntryModifiedAt,
  formatEntrySize,
} from '../entry-formatting/entry-formatting';
import { copyText } from './clipboard';
import { type FileViewerMetadata, mapLinkFor } from './file-metadata';
import type {
  FileViewerEpubSearchState,
  FileViewerPdfSearchState,
  FileViewerSearchState,
  FileViewerState,
} from './file-viewer-controller';
import { STRUCTURED_SORT_MAX_BYTES, TEXT_WINDOW_BYTES } from './file-viewer-controller';
import { searchHtml } from './html-search-highlight';
import { renderMarkdownWithHighlight } from './markdown-search-highlight';
import { type PDFDocumentProxy, renderPdfPageToCanvas } from './pdf-preview';
import './file-viewer.css';

/** Copies `value` to the clipboard and reports success/failure via toast - the same feedback
 * mechanism used elsewhere in the app (e.g. diagnostics' "Copy for Bug Report"), so the F3 viewer
 * doesn't invent a second, silent copy affordance. */
async function copyWithToast(action: () => Promise<void>, successMessage: string): Promise<void> {
  try {
    await action();
    toast({ html: successMessage });
  } catch {
    toast({ html: t('viewer', 'copyFailed') });
  }
}

/** Presentational Lister-style large-file viewer (task 0088); all state/async work lives in
 * `createFileViewerController` - this component only renders `attrs.state` and forwards intent
 * via callbacks, per this repo's convention of keeping application logic out of components. */
export interface FileViewerAttrs {
  readonly state: FileViewerState;
  readonly onLoadMore: () => void;
  readonly onLoadPrevious: () => void;
  readonly onLoadTextPage: (pageIndex: number) => void;
  readonly onLoadStructuredRows: (startRow: number) => void;
  readonly onStructuredOptionsChange: (
    delimiter: string,
    headerMode: 'auto' | 'firstRow' | 'none',
  ) => void;
  readonly onSelectStructuredSheet: (sheetName: string) => void;
  readonly onToggleStructuredRowNumbers: () => void;
  readonly onLoadJsonWindow: (offset: number) => void;
  readonly onSearchStructuredRows: (query: string, cursor?: number) => void;
  readonly onSortStructuredRows: (column: number) => void;
  readonly onSearchQueryChange: (query: string) => void;
  readonly onSearchOptionChange: (
    patch: Partial<Pick<FileViewerSearchState, 'regex' | 'caseSensitive' | 'wholeWord'>>,
  ) => void;
  readonly onRunSearch: () => void;
  readonly onNextMatch: () => void;
  readonly onPreviousMatch: () => void;
  readonly onZoomIn: () => void;
  readonly onZoomOut: () => void;
  readonly onZoomChange: (zoom: number) => void;
  readonly onResetZoom: () => void;
  readonly onCopy: () => Promise<void>;
  readonly onToggleMetadata: () => void;
  readonly onNextPage: () => void;
  readonly onPreviousPage: () => void;
  readonly onPdfSearchQueryChange: (query: string) => void;
  readonly onNextPdfMatch: () => void;
  readonly onPreviousPdfMatch: () => void;
  readonly onEpubSearchQueryChange: (query: string) => void;
  readonly onNextEpubMatch: () => void;
  readonly onPreviousEpubMatch: () => void;
  readonly onSelectEpubSection: (sectionIndex: number, fragment?: string) => void;
  readonly onFollowEpubLink: (href: string) => void;
  readonly onOpenExternalLink: (url: string) => void;
  readonly onSelectPdfPage: (pageNumber: number) => void;
  readonly onNavigateTextOffset: (offset: number, length: number) => void;
  readonly videoPosterDataUri?: string;
  readonly quickLookAvailable: boolean;
  readonly onQuickLook: () => void;
  readonly onOpenExternally: () => void;
  readonly onClose: () => void;
}

/** Renders markdown as sanitized HTML and, whenever a search match is highlighted, wraps it in a
 * `<mark>` and scrolls it into view via `renderMarkdownWithHighlight` - see that module's doc
 * comment for how it locates a match inside the rendered HTML. This owns the container's DOM
 * entirely and imperatively (like `code-mirror-editor.ts` owns its own) - `view()` below never
 * gives Mithril an `innerHTML` attribute to diff, so nothing here fights a Mithril redraw. A key
 * of `text`/`highlightOffset`/`highlightLength` skips re-rendering (and so re-scrolling) when
 * none of those actually changed since the last render (e.g. the user typed further in the
 * search box without navigating), mirroring `code-mirror-editor.ts`'s own guard against fighting
 * the user's manual scrolling. */
interface MarkdownPreviewAttrs {
  readonly text: string;
  readonly highlightOffset: number | undefined;
  readonly highlightLength: number | undefined;
}

const MarkdownPreview: FactoryComponent<MarkdownPreviewAttrs> = () => {
  let lastKey: string | undefined;
  function render(vnode: m.VnodeDOM<MarkdownPreviewAttrs>): void {
    const { text, highlightOffset, highlightLength } = vnode.attrs;
    const key = `${highlightOffset}:${highlightLength}:${text}`;
    if (key === lastKey) return;
    lastKey = key;
    renderMarkdownWithHighlight(
      vnode.dom as HTMLElement,
      safeMarkdownHtml(text),
      text,
      highlightOffset,
      highlightLength,
    );
  }
  return {
    oncreate: render,
    onupdate: render,
    view: () =>
      m('.fm-file-viewer-markdown.browser-default', {
        tabindex: 0,
        onkeydown: (event: KeyboardEvent) => {
          if (!(event.metaKey || event.ctrlKey) || event.altKey) return;
          const key = event.key.toLowerCase();
          if (key === 'a') {
            event.preventDefault();
            event.stopPropagation();
            const range = document.createRange();
            range.selectNodeContents(event.currentTarget as HTMLElement);
            const selection = document.getSelection();
            selection?.removeAllRanges();
            selection?.addRange(range);
          } else if (key === 'c') {
            event.stopPropagation();
          }
        },
      }),
  };
};

/** Normalizes the paged content kinds' differing field names (PDF's 1-based `currentPage`;
 * comic/EPUB's 0-based `currentPage`/`currentChapter`) into a single 1-based `{current, total}`
 * for the shared page-controls header UI. */
function pagedContentInfo(
  content: Extract<FileViewerState, { status: 'ready' }>['content'],
): { readonly current: number; readonly total: number } | undefined {
  if (content.kind === 'pdf') return { current: content.currentPage, total: content.pageCount };
  if (content.kind === 'comic')
    return { current: content.currentPage + 1, total: content.pageCount };
  if (content.kind === 'epub')
    return { current: content.currentChapter + 1, total: content.chapterCount };
  return undefined;
}

type ViewerOutlineItem = {
  readonly label: string;
  readonly level: number;
  readonly destination:
    | { readonly kind: 'epub'; readonly index: number; readonly fragment?: string }
    | { readonly kind: 'pdf'; readonly page: number }
    | { readonly kind: 'heading'; readonly index: number }
    | { readonly kind: 'text'; readonly offset: number; readonly length: number };
};

function htmlHeadingOutline(html: string): readonly ViewerOutlineItem[] {
  const doc = new DOMParser().parseFromString(html, 'text/html');
  return Array.from(doc.querySelectorAll('h1, h2, h3, h4, h5, h6')).flatMap((heading, index) => {
    const label = heading.textContent?.replace(/\s+/g, ' ').trim();
    if (label === undefined || label === '') return [];
    const level = Number(heading.tagName.slice(1));
    return [{ label, level, destination: { kind: 'heading' as const, index } }];
  });
}

function htmlSourceHeadingOutline(html: string): readonly ViewerOutlineItem[] {
  const headings: ViewerOutlineItem[] = [];
  for (const match of html.matchAll(/<h([1-6])\b[^>]*>([\s\S]*?)<\/h\1\s*>/giu)) {
    const label = new DOMParser()
      .parseFromString(match[2] ?? '', 'text/html')
      .body.textContent?.replace(/\s+/g, ' ')
      .trim();
    if (label === undefined || label === '') continue;
    headings.push({
      label,
      level: Number(match[1]),
      destination: {
        kind: 'text',
        offset: match.index,
        length: Math.max(1, match[0].length),
      },
    });
  }
  return headings;
}

function viewerOutline(
  state: Extract<FileViewerState, { status: 'ready' }>,
): readonly ViewerOutlineItem[] {
  const content = state.content;
  if (content.kind === 'epub') {
    if (content.outline !== undefined && content.outline.length > 0) {
      return content.outline.map((item) => ({
        label: item.label,
        level: item.level,
        destination: {
          kind: 'epub' as const,
          index: item.chapterIndex,
          ...(item.fragment === undefined ? {} : { fragment: item.fragment }),
        },
      }));
    }
    return content.sectionLabels.map((label, index) => ({
      label: label ?? t('viewer', 'sectionNumber', { number: index + 1 }),
      level: 1,
      destination: { kind: 'epub', index },
    }));
  }
  if (content.kind === 'pdf') {
    return (content.outline ?? []).map((item) => ({
      label: item.label,
      level: item.level,
      destination: { kind: 'pdf', page: item.page },
    }));
  }
  if (content.kind === 'docx') return htmlHeadingOutline(content.sourceHtml);
  if (content.kind !== 'text') return [];
  const language = editableLanguageForExtension(state.entry.extension, state.entry.name);
  if (language === 'markdown') return htmlHeadingOutline(safeMarkdownHtml(content.text));
  const extension = state.entry.extension?.toLowerCase();
  if (extension === 'html' || extension === 'htm') {
    return htmlSourceHeadingOutline(content.text);
  }
  return [];
}

function metadataField(label: string, value: string, href?: string): m.Children {
  return m('.fm-file-viewer-metadata-field', [
    m('dt', label),
    m('dd', [
      href === undefined
        ? m('span', value)
        : m('a', { href, target: '_blank', rel: 'noopener noreferrer' }, value),
      tooltip(
        t('viewer', 'copyLabel', { label }),
        m(
          'button.fm-file-viewer-metadata-copy',
          {
            type: 'button',
            'aria-label': t('viewer', 'copyLabel', { label }),
            onclick: () =>
              void copyWithToast(() => copyText(value), t('viewer', 'labelCopied', { label })),
          },
          copyIcon({ size: 12 }),
        ),
      ),
    ]),
  ]);
}

function renderMetadataPanel(metadata: FileViewerMetadata | 'loading' | undefined): m.Children {
  if (metadata === undefined) return undefined;
  if (metadata === 'loading') {
    return m('.fm-file-viewer-metadata', m('span', t('viewer', 'loadingMetadata')));
  }
  const fields: m.Children[] = [];
  if (metadata.kind === 'image') {
    if (metadata.width !== undefined && metadata.height !== undefined) {
      fields.push(
        metadataField(t('viewer', 'dimensions'), `${metadata.width} × ${metadata.height}`),
      );
    }
    fields.push(metadataField(t('viewer', 'type'), metadata.mimeType));
    if (metadata.sizeBytes !== undefined) {
      fields.push(
        metadataField(
          t('viewer', 'size'),
          formatEntrySize(
            { kind: 'file', size: metadata.sizeBytes },
            DEFAULT_ENTRY_FORMAT_SETTINGS,
          ),
        ),
      );
    }
    if (metadata.cameraMake !== undefined || metadata.cameraModel !== undefined) {
      fields.push(
        metadataField(
          t('viewer', 'camera'),
          [metadata.cameraMake, metadata.cameraModel].filter(Boolean).join(' '),
        ),
      );
    }
    if (metadata.dateTaken !== undefined) {
      fields.push(metadataField(t('viewer', 'dateTaken'), metadata.dateTaken));
    }
    if (metadata.gpsLatitude !== undefined && metadata.gpsLongitude !== undefined) {
      fields.push(
        metadataField(
          t('viewer', 'location'),
          `${metadata.gpsLatitude.toFixed(6)}, ${metadata.gpsLongitude.toFixed(6)}`,
          mapLinkFor(metadata.gpsLatitude, metadata.gpsLongitude),
        ),
      );
    }
  } else {
    fields.push(
      m('.fm-file-viewer-metadata-row', [
        metadataField(
          t('viewer', 'size'),
          metadata.sizeBytes === undefined
            ? '--'
            : formatEntrySize(
                { kind: 'file', size: metadata.sizeBytes },
                DEFAULT_ENTRY_FORMAT_SETTINGS,
              ),
        ),
        metadataField(
          metadata.windowedCount
            ? t('viewer', 'metadataLinesWindowed')
            : t('viewer', 'metadataLines'),
          String(metadata.lineCount),
        ),
        metadataField(
          metadata.windowedCount
            ? t('viewer', 'metadataCharactersWindowed')
            : t('viewer', 'metadataCharacters'),
          String(metadata.characterCount),
        ),
        metadataField(t('viewer', 'language'), metadata.language),
      ]),
    );
  }
  return m('.fm-file-viewer-metadata', m('dl', fields));
}

/** Renders the info panel's git history section (task 0135): commits touching this file, newest
 * first. Renders nothing while `gitHistory` is unset (panel closed, or the fetch hasn't started
 * yet) and nothing once resolved empty (the file has no history to show) - only a loading state
 * and a populated list are visible, so a plain file never grows an empty "History" heading. */
function renderGitHistorySection(
  gitHistory: readonly GitLogEntry[] | 'loading' | undefined,
): m.Children {
  if (gitHistory === undefined) return undefined;
  if (gitHistory === 'loading') {
    return m('.fm-file-viewer-git-history', m('span', t('viewer', 'loadingHistory')));
  }
  if (gitHistory.length === 0) return undefined;
  return m('.fm-file-viewer-git-history', [
    m('h4.fm-file-viewer-git-history-heading', t('viewer', 'history')),
    m(
      'ul.fm-file-viewer-git-history-list',
      gitHistory.map((commit) =>
        m('li.fm-file-viewer-git-history-entry', { key: commit.commitId }, [
          m('span.fm-file-viewer-git-history-summary', commit.summary),
          m(
            'span.fm-file-viewer-git-history-meta',
            t('viewer', 'gitHistoryMeta', {
              author: commit.authorName,
              date: formatEntryModifiedAt(commit.committedAt),
              shortId: commit.shortId,
            }),
          ),
        ]),
      ),
    ),
  ]);
}

function renderSearchBar(
  attrs: FileViewerAttrs,
  search: FileViewerSearchState | undefined,
  onInputRef: (el: HTMLInputElement) => void,
  onClose: () => void,
): m.Children {
  const query = search?.query ?? '';
  const matches = search?.matches ?? [];
  const currentMatchIndex = search?.currentMatchIndex;
  return m('.fm-file-viewer-search', [
    m('input.fm-file-viewer-search-input', {
      type: 'text',
      placeholder: t('viewer', 'searchPlaceholder'),
      value: query,
      'aria-label': t('viewer', 'searchThisFile'),
      oncreate: ({ dom }) => onInputRef(dom as HTMLInputElement),
      oninput: (event: InputEvent) =>
        attrs.onSearchQueryChange((event.currentTarget as HTMLInputElement).value),
      onkeydown: (event: KeyboardEvent) => {
        if (event.key === 'Escape') {
          onClose();
          return;
        }
        if (event.key === 'Enter') {
          event.preventDefault();
          attrs.onRunSearch();
        }
      },
    }),
    m(
      'button.fm-file-viewer-search-toggle',
      {
        type: 'button',
        title: t('viewer', 'matchCase'),
        'aria-pressed': search?.caseSensitive === true ? 'true' : 'false',
        onclick: () =>
          attrs.onSearchOptionChange({ caseSensitive: search?.caseSensitive !== true }),
      },
      'Aa',
    ),
    m(
      'button.fm-file-viewer-search-toggle',
      {
        type: 'button',
        title: t('viewer', 'matchWholeWord'),
        'aria-pressed': search?.wholeWord === true ? 'true' : 'false',
        onclick: () => attrs.onSearchOptionChange({ wholeWord: search?.wholeWord !== true }),
      },
      'Ab',
    ),
    m(
      'button.fm-file-viewer-search-toggle',
      {
        type: 'button',
        title: t('viewer', 'useRegex'),
        'aria-pressed': search?.regex === true ? 'true' : 'false',
        onclick: () => attrs.onSearchOptionChange({ regex: search?.regex !== true }),
      },
      '.*',
    ),
    m(
      'span.fm-file-viewer-search-count',
      search === undefined
        ? undefined
        : search.searching && matches.length === 0
          ? t('viewer', 'searching')
          : search.error !== undefined
            ? search.error
            : matches.length === 0
              ? query.trim() === ''
                ? undefined
                : t('viewer', 'noResults')
              : `${(currentMatchIndex ?? 0) + 1} of ${matches.length}${search.truncated ? '+' : ''}`,
    ),
    m(
      'button.fm-file-viewer-search-nav',
      {
        type: 'button',
        title: t('viewer', 'previousMatch'),
        disabled: matches.length === 0,
        onclick: attrs.onPreviousMatch,
      },
      '▲',
    ),
    m(
      'button.fm-file-viewer-search-nav',
      {
        type: 'button',
        title: t('viewer', 'nextMatch'),
        disabled: matches.length === 0,
        onclick: attrs.onNextMatch,
      },
      '▼',
    ),
    tooltip(
      t('viewer', 'closeSearch'),
      m(
        'button.fm-file-viewer-search-close',
        { type: 'button', 'aria-label': t('viewer', 'closeSearch'), onclick: onClose },
        closeIcon({ size: 13 }),
      ),
    ),
  ]);
}

function renderTextBody(
  attrs: FileViewerAttrs,
  state: Extract<FileViewerState, { status: 'ready' }>,
  onFindShortcut: () => void,
): m.Children {
  const content = state.content;
  if (content.kind !== 'text') return undefined;
  const editableLanguage = editableLanguageForExtension(state.entry.extension, state.entry.name);
  const paginated = !content.atStart || !content.atEnd;
  const knownOrMinimumBytes =
    state.entry.size ?? content.windowEnd + (content.atEnd ? 0 : TEXT_WINDOW_BYTES);
  const totalBytes = Math.max(knownOrMinimumBytes, content.windowEnd);
  const currentPage = Math.floor(content.windowOffset / TEXT_WINDOW_BYTES);
  return m('.fm-file-viewer-body.fm-file-viewer-body-text', {}, [
    editableLanguage === 'markdown'
      ? m(MarkdownPreview, {
          text: content.text,
          highlightOffset: content.highlightOffset,
          highlightLength: content.highlightLength,
        })
      : m(CodeMirrorEditor, {
          content: content.text,
          readOnly: true,
          language: languageExtension(editableLanguage),
          // The viewer only ever holds a lazily-loaded window of a large file in memory (see
          // `TEXT_WINDOW_BYTES`), so CodeMirror's own find panel - which only searches that
          // window - is disabled in favor of the toolbar search above, backed by a whole-file
          // backend scan. Mod-F still does something useful: it focuses that search box.
          enableBuiltInSearch: false,
          onFindShortcut,
          ...(content.highlightOffset === undefined || content.highlightLength === undefined
            ? {}
            : {
                selection: {
                  from: content.highlightOffset,
                  to: content.highlightOffset + content.highlightLength,
                },
              }),
        }),
    content.loadingMore ? m('.fm-file-viewer-loading-more', t('viewer', 'loadingMore')) : undefined,
    paginated
      ? m(
          '.fm-file-viewer-text-pagination',
          m(PaginationControls, {
            pagination: {
              page: currentPage,
              pageSize: TEXT_WINDOW_BYTES,
              total: totalBytes,
            },
            onPaginationChange: ({ page }) => attrs.onLoadTextPage(page),
          }),
        )
      : undefined,
  ]);
}

const STRUCTURED_ROW_HEIGHT = 20;
const STRUCTURED_COLUMN_WIDTH = 180;
const STRUCTURED_ROW_NUMBER_DIGIT_WIDTH = 7;
const STRUCTURED_ROW_NUMBER_PADDING = 8;

function structuredSortIndicator(direction: 'ascending' | 'descending'): m.Children {
  return m(
    'svg.fm-sort-indicator',
    {
      'aria-hidden': 'true',
      viewBox: '0 0 16 16',
      width: 12,
      height: 12,
    },
    m('path', {
      d: direction === 'ascending' ? 'M4 9 8 5l4 4' : 'M4 7l4 4 4-4',
      fill: 'none',
      stroke: 'currentColor',
      'stroke-width': 1.5,
      'stroke-linecap': 'round',
      'stroke-linejoin': 'round',
    }),
  );
}

const StructuredTable: FactoryComponent<{
  readonly attrs: FileViewerAttrs;
  readonly state: Extract<FileViewerState, { status: 'ready' }>;
}> = () => {
  let scrollTop = 0;
  let scrollLeft = 0;
  let viewportHeight = 480;
  let viewportWidth = 800;
  return {
    view: ({ attrs: wrapper }) => {
      const { attrs, state } = wrapper;
      if (state.content.kind !== 'structuredTable') return undefined;
      const content = state.content;
      const workbook = (content.sheets?.length ?? 0) > 0;
      const largestRowNumber = content.totalRows ?? content.indexedRows;
      const rowNumberWidth =
        content.showRowNumbers === true
          ? Math.max(1, String(largestRowNumber).length) * STRUCTURED_ROW_NUMBER_DIGIT_WIDTH +
            STRUCTURED_ROW_NUMBER_PADDING
          : 0;
      const sortEnabled =
        content.searchQuery !== '' ||
        (content.indexingComplete && content.sourceBytes <= STRUCTURED_SORT_MAX_BYTES);
      const sortDisabledReason = content.indexingComplete
        ? t('viewer', 'structuredSortLimit', {
            size: `${Math.round(STRUCTURED_SORT_MAX_BYTES / 1024)} KiB`,
          })
        : t('viewer', 'structuredSortAfterIndex');
      const visible = calculateVisibleWindow({
        entryCount: content.indexedRows,
        rowHeight: STRUCTURED_ROW_HEIGHT,
        scrollTop,
        viewportHeight,
        overscan: 6,
      });
      const columnCount = Math.max(
        content.headers.length,
        ...content.rows.map((row) => row.cells.length),
        1,
      );
      const dataScrollLeft = Math.max(0, scrollLeft - rowNumberWidth);
      const columnStart = Math.max(0, Math.floor(dataScrollLeft / STRUCTURED_COLUMN_WIDTH) - 2);
      const columnEnd = Math.min(
        columnCount,
        Math.ceil((dataScrollLeft + viewportWidth) / STRUCTURED_COLUMN_WIDTH) + 2,
      );
      const visibleRows = content.rows.filter(
        (row) => row.index >= visible.start && row.index < visible.end,
      );
      if (
        !content.loadingRows &&
        content.indexedRows > 0 &&
        (visible.start < content.rowStart || visible.end > content.rowStart + content.rows.length)
      ) {
        queueMicrotask(() => attrs.onLoadStructuredRows(visible.start));
      }
      const columns = Array.from(
        { length: columnEnd - columnStart },
        (_, index) => columnStart + index,
      );
      return m('.fm-structured-view', [
        m('.fm-structured-toolbar', [
          m('.fm-structured-search', [
            searchIcon({ size: 14, className: 'fm-structured-search-icon' }),
            m('input', {
              type: 'search',
              value: content.searchQuery,
              placeholder: t('viewer', 'structuredSearchRows'),
              oninput: (event: InputEvent) =>
                attrs.onSearchStructuredRows((event.currentTarget as HTMLInputElement).value),
            }),
          ]),
          workbook
            ? undefined
            : m(Select<string>, {
                className: 'fm-structured-option',
                appearance: 'outlined',
                label: t('viewer', 'structuredDelimiter'),
                options: [
                  { id: ',', label: t('viewer', 'structuredComma') },
                  { id: ';', label: t('viewer', 'structuredSemicolon') },
                  { id: '\t', label: t('viewer', 'structuredTab') },
                  { id: '|', label: t('viewer', 'structuredPipe') },
                ],
                checkedId: content.delimiter,
                onchange: (values: string[]) => {
                  const delimiter = values[0];
                  if (delimiter !== undefined)
                    attrs.onStructuredOptionsChange(delimiter, content.headerMode);
                },
              }),
          workbook
            ? undefined
            : m(Select<'auto' | 'firstRow' | 'none'>, {
                className: 'fm-structured-option',
                appearance: 'outlined',
                label: t('viewer', 'structuredHeader'),
                options: [
                  { id: 'auto' as const, label: t('viewer', 'structuredAuto') },
                  { id: 'firstRow' as const, label: t('viewer', 'structuredFirstRow') },
                  { id: 'none' as const, label: t('viewer', 'structuredNoHeader') },
                ],
                checkedId: content.headerMode,
                onchange: (values: ('auto' | 'firstRow' | 'none')[]) => {
                  const headerMode = values[0];
                  if (headerMode !== undefined)
                    attrs.onStructuredOptionsChange(content.delimiter, headerMode);
                },
              }),
          m(
            'span.fm-structured-progress',
            content.indexingComplete
              ? t('viewer', 'structuredRows', {
                  count: (content.totalRows ?? content.indexedRows).toLocaleString(),
                })
              : t('viewer', 'structuredIndexedRows', {
                  count: content.indexedRows.toLocaleString(),
                }),
          ),
        ]),
        content.warning === undefined ? undefined : m('.fm-structured-warning', content.warning),
        m('.fm-structured-header-viewport', [
          m(
            '.fm-structured-header-row',
            {
              style: {
                width: `${rowNumberWidth + columnCount * STRUCTURED_COLUMN_WIDTH}px`,
                transform: `translateX(${-scrollLeft}px)`,
              },
            },
            [
              content.showRowNumbers === true
                ? m(
                    '.fm-structured-cell.fm-structured-header-cell.fm-structured-header-row-number',
                    {
                      style: {
                        left: '0',
                        width: `${rowNumberWidth}px`,
                      },
                    },
                    '#',
                  )
                : undefined,
              columns.map((column) =>
                m(
                  '.fm-structured-cell.fm-structured-header-cell',
                  {
                    style: {
                      left: `${rowNumberWidth + column * STRUCTURED_COLUMN_WIDTH}px`,
                      width: `${STRUCTURED_COLUMN_WIDTH}px`,
                    },
                  },
                  m(
                    'button.fm-structured-sort',
                    {
                      type: 'button',
                      disabled: !sortEnabled,
                      'aria-sort':
                        content.sortColumn === column ? content.sortDirection : undefined,
                      title: sortEnabled
                        ? content.sortColumn === column && content.sortDirection === 'descending'
                          ? t('viewer', 'structuredClearSort')
                          : t('viewer', 'structuredSortColumn')
                        : sortDisabledReason,
                      onclick: () => attrs.onSortStructuredRows(column),
                    },
                    [
                      content.headers[column] ??
                        t('viewer', 'structuredColumn', { number: column + 1 }),
                      content.sortColumn === column && content.sortDirection !== undefined
                        ? structuredSortIndicator(content.sortDirection)
                        : undefined,
                    ],
                  ),
                ),
              ),
            ],
          ),
        ]),
        m(
          '.fm-structured-scroll',
          {
            onscroll: (event: Event) => {
              const target = event.currentTarget as HTMLElement;
              scrollTop = target.scrollTop;
              scrollLeft = target.scrollLeft;
              viewportHeight = target.clientHeight;
              viewportWidth = target.clientWidth;
            },
          },
          m(
            '.fm-structured-canvas',
            {
              style: {
                height: `${visible.totalHeight}px`,
                width: `${rowNumberWidth + columnCount * STRUCTURED_COLUMN_WIDTH}px`,
              },
            },
            visibleRows.map((row) =>
              m(
                '.fm-structured-row',
                {
                  key: row.index,
                  style: {
                    top: `${row.index * STRUCTURED_ROW_HEIGHT}px`,
                    height: `${STRUCTURED_ROW_HEIGHT}px`,
                  },
                  'data-row-stripe': row.index % 2 === 1 ? 'alternate' : undefined,
                },
                [
                  content.showRowNumbers === true
                    ? m(
                        '.fm-structured-cell.fm-structured-row-number-cell',
                        {
                          style: {
                            left: '0',
                            width: `${rowNumberWidth}px`,
                          },
                        },
                        row.index + 1,
                      )
                    : undefined,
                  columns.map((column) =>
                    (() => {
                      const detail = row.cellDetails?.find((cell) => cell.column === column);
                      const formula = detail?.formula ?? undefined;
                      const title =
                        formula === undefined
                          ? (row.cells[column] ?? '')
                          : t('viewer', 'structuredFormulaValue', {
                              formula,
                              value: detail?.display ?? row.cells[column] ?? '',
                            });
                      return m(
                        '.fm-structured-cell',
                        {
                          style: {
                            left: `${rowNumberWidth + column * STRUCTURED_COLUMN_WIDTH}px`,
                            width: `${STRUCTURED_COLUMN_WIDTH}px`,
                          },
                          title,
                          'data-value-type': detail?.valueType,
                        },
                        row.cells[column] ?? '',
                      );
                    })(),
                  ),
                ],
              ),
            ),
          ),
        ),
        workbook
          ? m('.fm-structured-sheet-tabs', [
              m(
                '.fm-structured-sheet-tab-list',
                content.sheets?.map((sheet) =>
                  m(
                    'button.fm-structured-sheet-tab',
                    {
                      type: 'button',
                      className: sheet.name === content.selectedSheet ? 'active' : undefined,
                      'aria-pressed': sheet.name === content.selectedSheet,
                      onclick: () => attrs.onSelectStructuredSheet(sheet.name),
                    },
                    sheet.name,
                  ),
                ),
              ),
              tooltip(
                t('viewer', 'structuredShowRowNumbers'),
                m('label.fm-structured-row-number-toggle', [
                  m('span', t('viewer', 'structuredRowsToggle')),
                  m('input.browser-default', {
                    type: 'checkbox',
                    checked: content.showRowNumbers === true,
                    'aria-label': t('viewer', 'structuredShowRowNumbers'),
                    onchange: attrs.onToggleStructuredRowNumbers,
                  }),
                ]),
                {
                  className: 'fm-structured-row-number-control',
                  'data-tooltip-placement': 'above',
                },
              ),
            ])
          : undefined,
      ]);
    },
  };
};

function renderStructuredJson(
  attrs: FileViewerAttrs,
  state: Extract<FileViewerState, { status: 'ready' }>,
): m.Children {
  if (state.content.kind !== 'structuredJson') return undefined;
  const content = state.content;
  const bytes = Uint8Array.from(content.data);
  const decoder = new TextDecoder();
  const parts: m.Children[] = [];
  let cursor = 0;
  for (const token of content.tokens) {
    if (token.start > cursor) parts.push(decoder.decode(bytes.slice(cursor, token.start)));
    const end = token.start + token.length;
    parts.push(
      m(`span.fm-json-token-${token.kind}`, {}, decoder.decode(bytes.slice(token.start, end))),
    );
    cursor = end;
  }
  if (cursor < bytes.length) parts.push(decoder.decode(bytes.slice(cursor)));
  return m('.fm-structured-json', [
    m('.fm-file-viewer-window-controls', [
      m(FlatButton, {
        className: 'fm-structured-window-previous',
        label: '<',
        disabled: content.atStart,
        tooltip: t('viewer', 'previousWindow'),
        onclick: () => attrs.onLoadJsonWindow(Math.max(0, content.windowOffset - 64 * 1024)),
      }),
      m(
        'span',
        `${content.windowOffset.toLocaleString()}–${(content.windowOffset + bytes.length).toLocaleString()} / ${content.sourceBytes.toLocaleString()} bytes`,
      ),
      m(FlatButton, {
        className: 'fm-structured-window-next',
        label: '>',
        disabled: content.atEnd,
        tooltip: t('viewer', 'nextWindow'),
        onclick: () => attrs.onLoadJsonWindow(content.windowOffset + bytes.length),
      }),
    ]),
    content.warning === undefined ? undefined : m('.fm-structured-warning', content.warning),
    m('pre.fm-structured-json-source.fm-structured-json-source-wrap', parts),
  ]);
}

function renderImageBody(
  attrs: FileViewerAttrs,
  state: Extract<FileViewerState, { status: 'ready' }>,
): m.Children {
  const content = state.content;
  if (content.kind !== 'image') return undefined;
  return m(
    '.fm-file-viewer-body.fm-file-viewer-body-image',
    {
      onwheel: (event: WheelEvent) => {
        event.preventDefault();
        if (event.deltaY < 0) attrs.onZoomIn();
        else if (event.deltaY > 0) attrs.onZoomOut();
      },
    },
    m('img', {
      src: content.dataUri,
      alt: state.entry.name,
      class: content.fitToContainer ? 'fm-file-viewer-image-fit' : undefined,
      style: content.fitToContainer ? undefined : { width: `${content.zoom * 100}%` },
    }),
  );
}

/** Renders one PDF page onto a canvas via pdf.js, scaled to fit the container on both axes.
 * Re-renders whenever `pageNumber` changes - tracked in local component state (`renderedPage`)
 * and checked from both `oncreate` and `onupdate`, rather than relying on Mithril's keyed-vnode
 * remount: a single, non-array child (as this is, inside `.fm-file-viewer-body-pdf`) only calls
 * `onupdate` on prop changes, never a fresh `oncreate`, so a `key`-only approach left page
 * navigation only moving the header's page counter without ever redrawing the canvas. Also
 * re-renders the current page (at the new size) whenever the container is resized, via
 * `ResizeObserver`, so the page keeps fitting the window rather than staying pinned to whatever
 * size the viewer happened to be when the page was first drawn. Surfaces render failures as text
 * instead of silently leaving the canvas blank, since a bad page (or a transient decode error) is
 * otherwise indistinguishable from "still loading". */
const PdfPageCanvas: FactoryComponent<{
  readonly document: PDFDocumentProxy;
  readonly pageNumber: number;
}> = () => {
  let renderedPage: number | undefined;
  let error: string | undefined;
  let resizeObserver: ResizeObserver | undefined;
  // The canvas element itself persists across page navigation (no key remount - see the class
  // doc comment), so `oncreate` only fires once; the resize observer it sets up there must read
  // the *current* attrs on every resize, not the ones captured when it was created.
  let latestAttrs: { document: PDFDocumentProxy; pageNumber: number } | undefined;
  function render(
    canvas: HTMLCanvasElement,
    attrs: { document: PDFDocumentProxy; pageNumber: number },
  ): void {
    renderedPage = attrs.pageNumber;
    error = undefined;
    const container = canvas.parentElement;
    const width = container?.clientWidth ?? 800;
    const height = container?.clientHeight ?? 1000;
    renderPdfPageToCanvas(attrs.document, attrs.pageNumber, canvas, width, height).catch(
      (cause: unknown) => {
        renderedPage = undefined;
        error = cause instanceof Error ? cause.message : t('viewer', 'failedToRenderPage');
        m.redraw();
      },
    );
  }
  function renderIfPageChanged(
    canvas: HTMLCanvasElement,
    attrs: { document: PDFDocumentProxy; pageNumber: number },
  ): void {
    latestAttrs = attrs;
    if (renderedPage === attrs.pageNumber) return;
    render(canvas, attrs);
  }
  return {
    view: ({ attrs }) =>
      error !== undefined
        ? m('.fm-file-viewer-pdf-page-error', `Couldn't render page ${attrs.pageNumber}: ${error}`)
        : m('canvas.fm-file-viewer-pdf-canvas', {
            oncreate: (vnode) => {
              const canvas = vnode.dom as HTMLCanvasElement;
              renderIfPageChanged(canvas, attrs);
              if (typeof ResizeObserver === 'function' && canvas.parentElement !== null) {
                resizeObserver = new ResizeObserver(() => {
                  if (latestAttrs !== undefined) render(canvas, latestAttrs);
                });
                resizeObserver.observe(canvas.parentElement);
              }
            },
            onupdate: (vnode) => renderIfPageChanged(vnode.dom as HTMLCanvasElement, attrs),
            onremove: () => {
              resizeObserver?.disconnect();
              resizeObserver = undefined;
            },
          }),
  };
};

function renderPagedSearchBar(
  search: FileViewerPdfSearchState | FileViewerEpubSearchState | undefined,
  placeholder: string,
  matchLabel: string,
  onQueryChange: (query: string) => void,
  onPreviousMatch: () => void,
  onNextMatch: () => void,
  onOptionChange: FileViewerAttrs['onSearchOptionChange'],
  onClose: () => void,
): m.Children {
  const query = search?.query ?? '';
  const matches = search?.matches ?? [];
  const currentMatchIndex = search?.currentMatchIndex;
  const currentMatch = matches[currentMatchIndex ?? 0];
  const matchPosition =
    typeof currentMatch === 'number' ? currentMatch : currentMatch?.chapterNumber;
  return m('.fm-file-viewer-search', [
    m('input.fm-file-viewer-search-input', {
      type: 'text',
      placeholder,
      value: query,
      'aria-label': placeholder,
      oninput: (event: InputEvent) =>
        onQueryChange((event.currentTarget as HTMLInputElement).value),
      onkeydown: (event: KeyboardEvent) => {
        if (event.key === 'Escape') onClose();
      },
    }),
    m(
      'button.fm-file-viewer-search-toggle',
      {
        type: 'button',
        title: t('viewer', 'matchCase'),
        'aria-pressed': search?.caseSensitive === true ? 'true' : 'false',
        onclick: () => onOptionChange({ caseSensitive: search?.caseSensitive !== true }),
      },
      'Aa',
    ),
    m(
      'button.fm-file-viewer-search-toggle',
      {
        type: 'button',
        title: t('viewer', 'matchWholeWord'),
        'aria-pressed': search?.wholeWord === true ? 'true' : 'false',
        onclick: () => onOptionChange({ wholeWord: search?.wholeWord !== true }),
      },
      'Ab',
    ),
    m(
      'button.fm-file-viewer-search-toggle',
      {
        type: 'button',
        title: t('viewer', 'useRegex'),
        'aria-pressed': search?.regex === true ? 'true' : 'false',
        onclick: () => onOptionChange({ regex: search?.regex !== true }),
      },
      '.*',
    ),
    m(
      'span.fm-file-viewer-search-count',
      search === undefined
        ? undefined
        : search.searching
          ? t('viewer', 'searching')
          : search.error !== undefined
            ? search.error
            : matches.length === 0
              ? query.trim() === ''
                ? undefined
                : t('viewer', 'noResults')
              : `${matchLabel} ${matchPosition} · ${(currentMatchIndex ?? 0) + 1} of ${matches.length}${search.searching ? '+' : ''}`,
    ),
    m(
      'button.fm-file-viewer-search-nav',
      {
        type: 'button',
        title: t('viewer', 'previousMatch'),
        disabled: matches.length === 0,
        onclick: onPreviousMatch,
      },
      '▲',
    ),
    m(
      'button.fm-file-viewer-search-nav',
      {
        type: 'button',
        title: t('viewer', 'nextMatch'),
        disabled: matches.length === 0,
        onclick: onNextMatch,
      },
      '▼',
    ),
    tooltip(
      t('viewer', 'closeSearch'),
      m(
        'button.fm-file-viewer-search-close',
        { type: 'button', 'aria-label': t('viewer', 'closeSearch'), onclick: onClose },
        closeIcon({ size: 13 }),
      ),
    ),
  ]);
}

function renderPdfBody(state: Extract<FileViewerState, { status: 'ready' }>): m.Children {
  const content = state.content;
  if (content.kind !== 'pdf') return undefined;
  return m(
    '.fm-file-viewer-body.fm-file-viewer-body-pdf',
    m(PdfPageCanvas, { document: content.document, pageNumber: content.currentPage }),
  );
}

function renderComicBody(state: Extract<FileViewerState, { status: 'ready' }>): m.Children {
  const content = state.content;
  if (content.kind !== 'comic') return undefined;
  return m(
    '.fm-file-viewer-body.fm-file-viewer-body-image',
    content.currentPageDataUri === undefined
      ? m('span', t('viewer', 'loadingPage'))
      : m('img.fm-file-viewer-image-fit', {
          src: content.currentPageDataUri,
          alt: `Page ${content.currentPage + 1} of ${state.entry.name}`,
        }),
  );
}

function externalEpubUrl(href: string): string | undefined {
  const trimmed = href.trim();
  const candidate = trimmed.startsWith('//') ? `https:${trimmed}` : trimmed;
  try {
    const url = new URL(candidate);
    return url.protocol === 'http:' || url.protocol === 'https:' || url.protocol === 'mailto:'
      ? url.href
      : undefined;
  } catch {
    return undefined;
  }
}

function renderEpubBody(
  attrs: FileViewerAttrs,
  state: Extract<FileViewerState, { status: 'ready' }>,
  requestExternalLink: (url: string) => void,
): m.Children {
  const content = state.content;
  if (content.kind !== 'epub') return undefined;
  let chapterHtml = content.currentChapterHtml;
  const search = state.epubSearch;
  if (chapterHtml !== undefined && search !== undefined && search.query !== '') {
    try {
      const activeMatch =
        search.currentMatchIndex === undefined ||
        search.matches[search.currentMatchIndex]?.chapterNumber !== content.currentChapter + 1
          ? undefined
          : search.matches[search.currentMatchIndex]?.occurrenceIndex;
      chapterHtml = searchHtml(
        chapterHtml,
        search.query,
        search.regex === true,
        search.caseSensitive === true,
        search.wholeWord === true,
        activeMatch,
      ).html;
    } catch {
      // The controller reports malformed regexes; preserve readable chapter HTML meanwhile.
    }
  }
  const revealTarget = (vnode: m.VnodeDOM): void => {
    if (content.targetFragment !== undefined) {
      let fragment = content.targetFragment;
      try {
        fragment = decodeURIComponent(fragment);
      } catch {
        // Keep the literal fragment when an EPUB contains malformed percent encoding.
      }
      (vnode.dom as Element).querySelector(`#${CSS.escape(fragment)}`)?.scrollIntoView({
        block: 'start',
        inline: 'nearest',
      });
      return;
    }
    const activeMatch = (vnode.dom as Element).querySelector('.fm-document-search-match-active');
    if (activeMatch instanceof HTMLElement && typeof activeMatch.scrollIntoView === 'function') {
      activeMatch.scrollIntoView({ block: 'center', inline: 'nearest' });
    }
  };
  return m(
    '.fm-file-viewer-body.fm-file-viewer-body-epub',
    {
      tabindex: 0,
      'data-viewer-focus-target': true,
      onclick: (event: MouseEvent) => {
        const anchor =
          event.target instanceof Element
            ? event.target.closest<HTMLAnchorElement>('a[href]')
            : null;
        if (anchor === null) return;
        const href = anchor.getAttribute('href');
        if (href === null) return;
        event.preventDefault();
        const externalUrl = externalEpubUrl(href);
        if (externalUrl === undefined) attrs.onFollowEpubLink(href);
        else requestExternalLink(externalUrl);
      },
    },
    chapterHtml === undefined
      ? m('span', t('viewer', 'loadingChapter'))
      : m('.fm-file-viewer-epub-chapter.browser-default', {
          innerHTML: chapterHtml,
          style: { fontSize: `${content.zoom}em` },
          oncreate: revealTarget,
          onupdate: revealTarget,
        }),
  );
}

function renderDocxBody(state: Extract<FileViewerState, { status: 'ready' }>): m.Children {
  const content = state.content;
  if (content.kind !== 'docx') return undefined;
  const revealActiveMatch = (vnode: m.VnodeDOM): void => {
    (vnode.dom as Element)
      .querySelector('.fm-document-search-match-active')
      ?.scrollIntoView({ block: 'center', inline: 'nearest' });
  };
  return m('.fm-file-viewer-body.fm-file-viewer-body-docx', [
    m('.fm-file-viewer-docx-content.browser-default', {
      innerHTML: content.html,
      oncreate: revealActiveMatch,
      onupdate: revealActiveMatch,
    }),
    content.omittedFeatures.length === 0
      ? undefined
      : m(
          'p.fm-file-viewer-docx-limitations',
          t('viewer', 'docxContentOnly', { features: content.omittedFeatures.join(', ') }),
        ),
  ]);
}

function renderAudioBody(state: Extract<FileViewerState, { status: 'ready' }>): m.Children {
  const content = state.content;
  if (content.kind !== 'audio') return undefined;
  return m(
    '.fm-file-viewer-body.fm-file-viewer-body-audio',
    m('audio', {
      controls: true,
      autoplay: false,
      src: content.dataUri,
      'aria-label': state.entry.name,
    }),
  );
}

function formatArchiveBytes(bytes: number): string {
  if (bytes === 0) return '0 B';
  const units = ['B', 'KB', 'MB', 'GB', 'TB'];
  const unit = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** unit;
  return `${new Intl.NumberFormat(undefined, { maximumFractionDigits: unit === 0 ? 0 : 1 }).format(value)} ${units[unit]}`;
}

function renderArchiveSummary(state: Extract<FileViewerState, { status: 'ready' }>): m.Children {
  const content = state.content;
  if (content.kind !== 'archiveSummary') return undefined;
  const compressionRatio =
    content.compressedSize === undefined || content.compressedSize === 0
      ? t('viewer', 'archiveNotAvailable')
      : `${new Intl.NumberFormat(undefined, { maximumFractionDigits: 1 }).format(
          content.uncompressedSize / content.compressedSize,
        )}:1`;
  const fields = [
    [t('viewer', 'archiveFormat'), content.format.toLocaleUpperCase()],
    [t('viewer', 'archiveFiles'), t('viewer', 'archiveFileCount', content.fileCount)],
    [
      t('viewer', 'archiveDirectories'),
      t('viewer', 'archiveDirectoryCount', content.directoryCount),
    ],
    [t('viewer', 'archiveUncompressedSize'), formatArchiveBytes(content.uncompressedSize)],
    [
      t('viewer', 'archiveCompressedSize'),
      content.compressedSize === undefined
        ? t('viewer', 'archiveNotAvailable')
        : formatArchiveBytes(content.compressedSize),
    ],
    [t('viewer', 'archiveCompressionRatio'), compressionRatio],
  ] as const;
  return m(
    '.fm-file-viewer-body.fm-file-viewer-archive-summary',
    m(
      'dl',
      fields.map(([label, value]) =>
        m('.fm-file-viewer-archive-summary-field', [m('dt', label), m('dd', value)]),
      ),
    ),
  );
}

function renderVideoBody(
  attrs: FileViewerAttrs,
  state: Extract<FileViewerState, { status: 'ready' }>,
): m.Children {
  const content = state.content;
  if (content.kind === 'videoExternal') {
    return renderExternalFallback(attrs, t('viewer', 'videoExternalOnly'));
  }
  if (content.kind !== 'video') return undefined;
  return m(
    '.fm-file-viewer-body.fm-file-viewer-body-video',
    m('video', {
      controls: true,
      autoplay: false,
      src: content.dataUri,
      ...(attrs.videoPosterDataUri === undefined ? {} : { poster: attrs.videoPosterDataUri }),
      'aria-label': state.entry.name,
    }),
  );
}

function renderExternalFallback(attrs: FileViewerAttrs, message: string): m.Children {
  return m('.fm-file-viewer-body.fm-file-viewer-external-fallback', [
    m('p', message),
    m('.fm-file-viewer-fallback-actions', [
      attrs.quickLookAvailable
        ? m(
            'button.fm-file-viewer-quick-look',
            { type: 'button', onclick: attrs.onQuickLook },
            t('viewer', 'quickLook'),
          )
        : undefined,
      m(
        'button.fm-file-viewer-open-externally',
        { type: 'button', onclick: attrs.onOpenExternally },
        t('viewer', 'openExternally'),
      ),
    ]),
  ]);
}

export const FileViewer: FactoryComponent<FileViewerAttrs> = () => {
  let searchInput: HTMLInputElement | undefined;
  let editingPage = false;
  let pageInputValue = '';
  let editingZoom = false;
  let zoomInputValue = '';
  let searchOpen = false;
  let initialSearchVisibilitySet = false;
  let outlineOpen = false;
  let pendingExternalLink: string | undefined;
  let viewerElement: HTMLElement | undefined;
  const toggleSearch = (): void => {
    searchOpen = !searchOpen;
    m.redraw.sync();
    if (searchOpen) {
      searchInput =
        viewerElement?.querySelector<HTMLInputElement>('.fm-file-viewer-search-input') ?? undefined;
      searchInput?.focus();
    }
  };
  return {
    view: ({ attrs }) => {
      const state = attrs.state;
      if (!initialSearchVisibilitySet && state.status === 'ready') {
        initialSearchVisibilitySet = true;
        const initialQuery =
          state.content.kind === 'pdf'
            ? state.pdfSearch?.query
            : state.content.kind === 'epub'
              ? state.epubSearch?.query
              : state.search?.query;
        searchOpen = initialQuery !== undefined && initialQuery !== '';
      }
      const outline = state.status === 'ready' ? viewerOutline(state) : [];
      const search =
        state.status === 'ready' && (state.content.kind === 'text' || state.content.kind === 'docx')
          ? state.search
          : undefined;
      return m(
        'section.fm-file-viewer',
        {
          'aria-label': t('viewer', 'viewing', { name: state.entry.name }),
          oncreate: ({ dom }) => {
            viewerElement = dom as HTMLElement;
            viewerElement.addEventListener('fm-viewer-toggle-search', toggleSearch);
          },
          onremove: () => {
            viewerElement?.removeEventListener('fm-viewer-toggle-search', toggleSearch);
            viewerElement = undefined;
          },
        },
        [
          m('.fm-file-viewer-header', [
            outline.length > 0
              ? tooltip(
                  t('viewer', 'documentOutline'),
                  m(
                    'button.fm-file-viewer-outline-toggle',
                    {
                      type: 'button',
                      'aria-label': t('viewer', 'documentOutline'),
                      'aria-expanded': outlineOpen ? 'true' : 'false',
                      onclick: () => {
                        outlineOpen = !outlineOpen;
                      },
                    },
                    menuIcon({ size: 15 }),
                  ),
                )
              : undefined,
            m('strong.fm-file-viewer-title', state.entry.name),
            state.status === 'ready' &&
            (state.content.kind === 'image' || state.content.kind === 'epub')
              ? m('.fm-file-viewer-zoom-controls', [
                  tooltip(
                    t('viewer', 'zoomOut'),
                    m('button', { type: 'button', onclick: attrs.onZoomOut }, '−'),
                  ),
                  editingZoom
                    ? m('input.fm-file-viewer-zoom-input', {
                        type: 'text',
                        inputmode: 'numeric',
                        min: '10',
                        max: '800',
                        value: zoomInputValue,
                        'aria-label': t('viewer', 'zoomPercent'),
                        oncreate: ({ dom }) => (dom as HTMLInputElement).select(),
                        oninput: (event: InputEvent) => {
                          zoomInputValue = (event.currentTarget as HTMLInputElement).value;
                        },
                        onkeydown: (event: KeyboardEvent) => {
                          if (event.key === 'Escape') editingZoom = false;
                          if (event.key !== 'Enter') return;
                          const value = Number((event.currentTarget as HTMLInputElement).value);
                          if (Number.isFinite(value)) attrs.onZoomChange(value / 100);
                          editingZoom = false;
                        },
                        onblur: () => {
                          editingZoom = false;
                          zoomInputValue = '';
                        },
                      })
                    : m(
                        'button.fm-file-viewer-zoom-level',
                        {
                          type: 'button',
                          title: t('viewer', 'zoomPercent'),
                          onclick: () => {
                            const content =
                              attrs.state.status === 'ready' ? attrs.state.content : undefined;
                            if (content?.kind !== 'image' && content?.kind !== 'epub') return;
                            zoomInputValue = String(Math.round(content.zoom * 100));
                            editingZoom = true;
                          },
                        },
                        state.content.kind === 'image' && state.content.fitToContainer
                          ? t('viewer', 'fit')
                          : `${Math.round(state.content.zoom * 100)}%`,
                      ),
                  tooltip(
                    t('viewer', 'zoomIn'),
                    m('button', { type: 'button', onclick: attrs.onZoomIn }, '+'),
                  ),
                  tooltip(
                    t('viewer', 'fitToWindow'),
                    m('button', { type: 'button', onclick: attrs.onResetZoom }, t('viewer', 'fit')),
                  ),
                ])
              : undefined,
            state.status === 'ready' && pagedContentInfo(state.content) !== undefined
              ? (() => {
                  const pageInfo = pagedContentInfo(state.content);
                  if (pageInfo === undefined) return undefined;
                  return m('.fm-file-viewer-page-controls', [
                    tooltip(
                      t('viewer', 'previousPage'),
                      m(
                        'button',
                        {
                          type: 'button',
                          'aria-label': t('viewer', 'previousPage'),
                          disabled: pageInfo.current <= 1,
                          onclick: attrs.onPreviousPage,
                        },
                        '◀',
                      ),
                    ),
                    editingPage && (state.content.kind === 'epub' || state.content.kind === 'pdf')
                      ? m('.fm-file-viewer-page-editor', [
                          m('input.fm-file-viewer-page-input', {
                            type: 'text',
                            inputmode: 'numeric',
                            min: '1',
                            max: String(pageInfo.total),
                            value: pageInputValue,
                            'aria-label':
                              state.content.kind === 'epub'
                                ? t('viewer', 'selectSection')
                                : t('viewer', 'goToPage'),
                            oncreate: ({ dom }) => (dom as HTMLInputElement).select(),
                            oninput: (event: InputEvent) => {
                              pageInputValue = (event.currentTarget as HTMLInputElement).value;
                            },
                            onkeydown: (event: KeyboardEvent) => {
                              if (event.key === 'Escape') editingPage = false;
                              if (event.key !== 'Enter') return;
                              const value = Math.trunc(
                                Number((event.currentTarget as HTMLInputElement).value),
                              );
                              if (value >= 1 && value <= pageInfo.total) {
                                if (state.content.kind === 'epub') {
                                  attrs.onSelectEpubSection(value - 1);
                                } else {
                                  attrs.onSelectPdfPage(value);
                                }
                              }
                              editingPage = false;
                            },
                            onblur: () => {
                              editingPage = false;
                              pageInputValue = '';
                            },
                          }),
                          m('span', `/ ${pageInfo.total}`),
                        ])
                      : state.content.kind === 'epub' || state.content.kind === 'pdf'
                        ? m(
                            'button.fm-file-viewer-page-count',
                            {
                              type: 'button',
                              title:
                                state.content.kind === 'epub'
                                  ? t('viewer', 'selectSection')
                                  : t('viewer', 'goToPage'),
                              onclick: () => {
                                pageInputValue = String(pageInfo.current);
                                editingPage = true;
                              },
                            },
                            `${pageInfo.current} / ${pageInfo.total}`,
                          )
                        : m(
                            'span.fm-file-viewer-page-count',
                            `${pageInfo.current} / ${pageInfo.total}`,
                          ),
                    tooltip(
                      t('viewer', 'nextPage'),
                      m(
                        'button',
                        {
                          type: 'button',
                          'aria-label': t('viewer', 'nextPage'),
                          disabled: pageInfo.current >= pageInfo.total,
                          onclick: attrs.onNextPage,
                        },
                        '▶',
                      ),
                    ),
                  ]);
                })()
              : undefined,
            state.status === 'ready' &&
            (state.content.kind === 'text' || state.content.kind === 'image')
              ? tooltip(
                  state.content.kind === 'image'
                    ? t('viewer', 'copyImage')
                    : t('viewer', 'copyText'),
                  m(
                    'button.fm-file-viewer-copy',
                    {
                      type: 'button',
                      'aria-label':
                        state.content.kind === 'image'
                          ? t('viewer', 'copyImage')
                          : t('viewer', 'copyText'),
                      onclick: () =>
                        void copyWithToast(
                          attrs.onCopy,
                          state.content.kind === 'image'
                            ? t('viewer', 'imageCopied')
                            : t('viewer', 'textCopied'),
                        ),
                    },
                    copyIcon({ size: 15 }),
                  ),
                )
              : undefined,
            state.status === 'ready' &&
            (state.content.kind === 'text' || state.content.kind === 'image')
              ? tooltip(
                  t('viewer', 'showInfo'),
                  m(
                    'button.fm-file-viewer-metadata-toggle',
                    {
                      type: 'button',
                      'aria-label': t('viewer', 'showInfo'),
                      'aria-pressed': state.metadataPanelOpen === true ? 'true' : 'false',
                      onclick: attrs.onToggleMetadata,
                    },
                    infoCircleIcon({ size: 15 }),
                  ),
                )
              : undefined,
            tooltip(
              t('viewer', 'closeViewer'),
              m(
                'button.fm-file-viewer-close',
                {
                  type: 'button',
                  'aria-label': t('viewer', 'closeViewer'),
                  onclick: attrs.onClose,
                },
                closeIcon({ size: 13 }),
              ),
            ),
          ]),
          outlineOpen && outline.length > 0
            ? m(
                '.fm-file-viewer-outline',
                {
                  role: 'menu',
                  'aria-label': t('viewer', 'documentOutline'),
                  onkeydown: (event: KeyboardEvent) => {
                    if (event.key === 'Escape') outlineOpen = false;
                  },
                },
                outline.map((item, index) =>
                  m(
                    'button.fm-file-viewer-outline-item',
                    {
                      type: 'button',
                      role: 'menuitem',
                      title: item.label,
                      style: { paddingInlineStart: `${0.5 + (item.level - 1) * 0.75}rem` },
                      onclick: () => {
                        outlineOpen = false;
                        if (item.destination.kind === 'epub') {
                          if (item.destination.fragment === undefined) {
                            attrs.onSelectEpubSection(item.destination.index);
                          } else {
                            attrs.onSelectEpubSection(
                              item.destination.index,
                              item.destination.fragment,
                            );
                          }
                        } else if (item.destination.kind === 'pdf') {
                          attrs.onSelectPdfPage(item.destination.page);
                        } else if (item.destination.kind === 'text') {
                          attrs.onNavigateTextOffset(
                            item.destination.offset,
                            item.destination.length,
                          );
                        } else {
                          viewerElement
                            ?.querySelectorAll<HTMLElement>(
                              '.fm-file-viewer-markdown h1, .fm-file-viewer-markdown h2, .fm-file-viewer-markdown h3, .fm-file-viewer-markdown h4, .fm-file-viewer-markdown h5, .fm-file-viewer-markdown h6, .fm-file-viewer-docx-content h1, .fm-file-viewer-docx-content h2, .fm-file-viewer-docx-content h3, .fm-file-viewer-docx-content h4, .fm-file-viewer-docx-content h5, .fm-file-viewer-docx-content h6',
                            )
                            [item.destination.index]?.scrollIntoView({
                              block: 'start',
                              inline: 'nearest',
                            });
                        }
                      },
                    },
                    [
                      m('span.fm-file-viewer-outline-index', String(index + 1)),
                      m('span', item.label),
                    ],
                  ),
                ),
              )
            : undefined,
          searchOpen &&
          state.status === 'ready' &&
          (state.content.kind === 'text' || state.content.kind === 'docx')
            ? renderSearchBar(
                attrs,
                search,
                (el) => {
                  searchInput = el;
                },
                () => {
                  searchOpen = false;
                },
              )
            : undefined,
          searchOpen && state.status === 'ready' && state.content.kind === 'pdf'
            ? renderPagedSearchBar(
                state.pdfSearch,
                t('viewer', 'searchPdfPlaceholder'),
                'Page',
                attrs.onPdfSearchQueryChange,
                attrs.onPreviousPdfMatch,
                attrs.onNextPdfMatch,
                attrs.onSearchOptionChange,
                () => {
                  searchOpen = false;
                },
              )
            : undefined,
          searchOpen && state.status === 'ready' && state.content.kind === 'epub'
            ? renderPagedSearchBar(
                state.epubSearch,
                t('viewer', 'searchThisFile'),
                t('viewer', 'chapter'),
                attrs.onEpubSearchQueryChange,
                attrs.onPreviousEpubMatch,
                attrs.onNextEpubMatch,
                attrs.onSearchOptionChange,
                () => {
                  searchOpen = false;
                },
              )
            : undefined,
          state.status === 'loading'
            ? m('.fm-file-viewer-body', m('span', t('shell', 'loading')))
            : state.status === 'unsupported'
              ? renderExternalFallback(attrs, t('viewer', 'previewUnavailableGeneric'))
              : state.status === 'error'
                ? renderExternalFallback(attrs, state.message)
                : state.content.kind === 'text'
                  ? renderTextBody(attrs, state, () => searchInput?.focus())
                  : state.content.kind === 'structuredTable'
                    ? m(StructuredTable, { attrs, state })
                    : state.content.kind === 'structuredJson'
                      ? renderStructuredJson(attrs, state)
                      : state.content.kind === 'structuredFallback'
                        ? renderExternalFallback(attrs, state.content.message)
                        : state.content.kind === 'docxExternal' ||
                            state.content.kind === 'pptxExternal'
                          ? renderExternalFallback(attrs, state.content.message)
                          : state.content.kind === 'audio'
                            ? renderAudioBody(state)
                            : state.content.kind === 'video' ||
                                state.content.kind === 'videoExternal'
                              ? renderVideoBody(attrs, state)
                              : state.content.kind === 'pdf'
                                ? renderPdfBody(state)
                                : state.content.kind === 'comic'
                                  ? renderComicBody(state)
                                  : state.content.kind === 'epub'
                                    ? renderEpubBody(attrs, state, (url) => {
                                        pendingExternalLink = url;
                                      })
                                    : state.content.kind === 'docx'
                                      ? renderDocxBody(state)
                                      : state.content.kind === 'archiveSummary'
                                        ? renderArchiveSummary(state)
                                        : renderImageBody(attrs, state),
          state.status === 'ready' && state.metadataPanelOpen === true
            ? m('.fm-file-viewer-info-panel', [
                renderMetadataPanel(state.metadata),
                renderGitHistorySection(state.gitHistory),
              ])
            : undefined,
          m(ModalPanel, {
            title: t('viewer', 'openExternalLinkTitle'),
            description: m(
              'p',
              t('viewer', 'openExternalLinkMessage', { url: pendingExternalLink ?? '' }),
            ),
            isOpen: pendingExternalLink !== undefined,
            closeOnEsc: true,
            onToggle: (open: boolean) => {
              if (!open) pendingExternalLink = undefined;
            },
            buttons: [
              {
                label: t('button', 'cancel'),
                onclick: () => {
                  pendingExternalLink = undefined;
                },
              },
              {
                label: t('viewer', 'openLink'),
                onclick: () => {
                  const url = pendingExternalLink;
                  pendingExternalLink = undefined;
                  if (url !== undefined) attrs.onOpenExternalLink(url);
                },
              },
            ],
          }),
        ],
      );
    },
  };
};
