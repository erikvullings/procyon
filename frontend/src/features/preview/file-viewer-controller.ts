import type { FileManagerClient } from '../../api/client/file-manager-client';
import { t } from '../../i18n';
import type {
  EntrySummary,
  GitLogEntry,
  JsonTokenSpan,
  Location,
  SearchInFileMatch,
  StructuredRow,
} from '../../models';
import { IMAGE_EXTENSIONS } from '../directory-table/entry-icons';
import { editableLanguageForExtension } from '../editor/editor-language';
import { archiveEntryLocation, archiveRootForEntry } from '../navigation/archive-location';
import { copyImageDataUri, copyText } from './clipboard';
import {
  bytesToDataUri,
  imageMimeTypeFor,
  PREVIEW_SIZE_LIMIT_BYTES,
  readEntireFileBytes,
  readFullAudioDataUri,
  readFullImageDataUri,
  readFullVideoDataUri,
  resolvePreviewKind,
} from './content-preview';
import { prepareDocxPreviewHtml, searchDocxHtml } from './docx-preview';
import {
  type EpubImageResource,
  inlineEpubChapterImages,
  parseEpubContainer,
  parseEpubNavigationEntries,
  parseEpubPackage,
  parseEpubSectionLabels,
  repairEpubChapterOrder,
  resolveEpubPath,
  sanitizeEpubSvg,
} from './epub-preview';
import {
  type FileViewerMetadata,
  readImageDimensions,
  readImageExif,
  textMetadataFor,
} from './file-metadata';
import { loadPdfDocument, type PDFDocumentProxy } from './pdf-preview';

/** Client surface required to drive a Lister-style large-file viewer. `listDirectory` is only
 * used for comic (.cbz/.cbr) page listing. */
export type FileViewerClient = Pick<
  FileManagerClient,
  | 'readFileRange'
  | 'searchInFile'
  | 'listDirectory'
  | 'archiveSummary'
  | 'gitFileHistory'
  | 'openStructuredView'
  | 'getStructuredViewStatus'
  | 'updateStructuredView'
  | 'readStructuredRows'
  | 'readStructuredJsonWindow'
  | 'searchStructuredRows'
  | 'closeStructuredView'
  | 'openDocxPreview'
  | 'readDocxPreviewResource'
  | 'closeDocxPreview'
  | 'openPptxPreview'
  | 'readPptxPreviewPdf'
  | 'closePptxPreview'
>;

/** Bytes fetched per text window load (initial load and each "load more" append). */
export const TEXT_WINDOW_BYTES = 64 * 1024;
const PPTX_PDF_RANGE_BYTES = 1024 * 1024;

/** Inline video uses the existing preview limit because base64 data URIs expand bytes by roughly
 * one third and must be held in memory alongside the source buffer. Larger videos stay external. */
export const VIDEO_INLINE_SIZE_LIMIT_BYTES = PREVIEW_SIZE_LIMIT_BYTES;

/** Bytes of context fetched before a search match when jumping to it. */
const JUMP_CONTEXT_BEFORE_BYTES = TEXT_WINDOW_BYTES / 2;

const ZOOM_MIN = 0.1;
const ZOOM_MAX = 8;

/** Delay before an edit to the search query/options triggers a search, so rapid typing doesn't
 * fire one request per keystroke. */
const SEARCH_DEBOUNCE_MS = 200;

/** The currently loaded text window, and whether more can be loaded in either direction. */
export interface FileViewerTextContent {
  readonly kind: 'text';
  readonly windowOffset: number;
  readonly windowEnd: number;
  readonly text: string;
  readonly atStart: boolean;
  readonly atEnd: boolean;
  readonly loadingMore: boolean;
  /**
   * Character offset/length of the active search match within `text`, for scroll/highlight.
   *
   * The backend reports match positions as UTF-8 BYTE offsets (`SearchInFileMatch.offset`), which
   * do not equal JS string (UTF-16 code unit) offsets once any multi-byte character precedes the
   * match - using the raw byte offset directly made the highlight drift later and later into the
   * file, worsening with every prior non-ASCII character. These fields are already converted to
   * `text`-relative character positions (see `jumpToMatch`), so no further conversion is needed.
   */
  readonly highlightOffset?: number;
  readonly highlightLength?: number;
}

/** A bounded row page backed by a sparse-indexed structured-view session. */
export interface FileViewerStructuredTableContent {
  readonly kind: 'structuredTable';
  readonly sessionId: string;
  readonly sourceBytes: number;
  readonly delimiter: string;
  readonly headerMode: 'auto' | 'firstRow' | 'none';
  readonly headers: readonly string[];
  readonly rows: readonly StructuredRow[];
  readonly rowStart: number;
  readonly indexedRows: number;
  readonly totalRows: number | undefined;
  readonly sourceIndexedRows: number;
  readonly sourceTotalRows: number | undefined;
  readonly indexingComplete: boolean;
  readonly loadingRows: boolean;
  readonly warning: string | undefined;
  readonly sheets?: readonly {
    readonly name: string;
    readonly rowCount: number;
    readonly columnCount: number;
  }[];
  readonly selectedSheet?: string;
  readonly searchQuery: string;
  readonly searchMatches: readonly StructuredRow[];
  readonly searchNextCursor: number | undefined;
  readonly searching: boolean;
  readonly showRowNumbers?: boolean;
  readonly sortColumn?: number;
  readonly sortDirection?: 'ascending' | 'descending';
}

/** One bounded raw JSON window and its backend-produced byte-relative token spans. */
export interface FileViewerStructuredJsonContent {
  readonly kind: 'structuredJson';
  readonly sessionId: string;
  readonly data: readonly number[];
  readonly tokens: readonly JsonTokenSpan[];
  readonly windowOffset: number;
  readonly sourceBytes: number;
  readonly atStart: boolean;
  readonly atEnd: boolean;
  readonly loadingWindow: boolean;
  readonly warning: string | undefined;
}

/** Honest bounded fallback for workbook formats that would require materializing a sheet. */
export interface FileViewerStructuredFallbackContent {
  readonly kind: 'structuredFallback';
  readonly message: string;
}

/** The currently loaded (full) image and its zoom state. */
export interface FileViewerImageContent {
  readonly kind: 'image';
  readonly dataUri: string;
  readonly zoom: number;
  readonly fitToContainer: boolean;
}

/** The currently loaded (full) audio file, played back via the native `<audio>` element - which
 * reports its own duration/position, so no metadata needs fetching separately. */
export interface FileViewerAudioContent {
  readonly kind: 'audio';
  readonly dataUri: string;
}

/** A small, browser-playable video loaded through the same whole-file data URI path as audio. */
export interface FileViewerVideoContent {
  readonly kind: 'video';
  readonly dataUri: string;
}

/** A video that is unsafe or unreliable to load into an in-memory browser data URI. */
export interface FileViewerExternalVideoContent {
  readonly kind: 'videoExternal';
}

/** A loaded PDF document, rendered page-by-page onto a canvas by `PdfPageCanvas` (`file-viewer.ts`)
 * - the document proxy itself lives here so the view can call `document.getPage()` without the
 * controller owning canvas/DOM concerns. */
export interface FileViewerPdfContent {
  readonly kind: 'pdf';
  readonly document: PDFDocumentProxy;
  readonly pageCount: number;
  readonly currentPage: number;
  readonly outline?: readonly FileViewerPdfOutlineItem[];
}

export interface FileViewerPdfOutlineItem {
  readonly label: string;
  readonly level: number;
  readonly page: number;
}

/** A comic archive (.cbz/.cbr), paginated as its image entries in name order. Only the current
 * page's bytes are fetched (matching Total Commander's Lister, which never extracts a whole
 * archive just to view it) - `currentPageDataUri` is `undefined` while `loadingPage` is true. */
export interface FileViewerComicContent {
  readonly kind: 'comic';
  readonly pageCount: number;
  readonly currentPage: number;
  readonly currentPageDataUri: string | undefined;
  readonly loadingPage: boolean;
}

/** An EPUB, paginated as its spine's XHTML chapters in reading order. Only the current chapter's
 * sanitized HTML is kept (matching the comic/PDF "one page's worth of content at a time"
 * approach) - `currentChapterHtml` is `undefined` while `loadingChapter` is true. Manifest images
 * referenced by the current chapter are fetched and inlined before its HTML is published. */
export interface FileViewerEpubContent {
  readonly kind: 'epub';
  readonly title: string | undefined;
  readonly chapterCount: number;
  readonly currentChapter: number;
  readonly currentChapterHtml: string | undefined;
  readonly loadingChapter: boolean;
  readonly zoom: number;
  readonly sectionLabels: readonly (string | undefined)[];
  readonly outline?: readonly FileViewerEpubOutlineItem[];
  readonly targetFragment?: string;
}

export interface FileViewerEpubOutlineItem {
  readonly label: string;
  readonly level: number;
  readonly chapterIndex: number;
  readonly fragment?: string;
}

/** Sanitized semantic DOCX content plus an honest list of omitted Word layout features. */
export interface FileViewerDocxContent {
  readonly kind: 'docx';
  readonly sessionId: string;
  readonly sourceHtml: string;
  readonly html: string;
  readonly plainText: string;
  readonly omittedFeatures: readonly string[];
}

/** DOCX content that exceeded a safety budget or could not be parsed safely. */
export interface FileViewerExternalDocxContent {
  readonly kind: 'docxExternal';
  readonly message: string;
}

/** PPTX content that exceeded a safety budget or could not be rendered safely. */
export interface FileViewerExternalPptxContent {
  readonly kind: 'pptxExternal';
  readonly message: string;
}

/** Content-derived archive details plus provider-neutral recursive totals. */
export interface FileViewerArchiveSummaryContent {
  readonly kind: 'archiveSummary';
  readonly format: string;
  readonly fileCount: number;
  readonly directoryCount: number;
  readonly uncompressedSize: number;
  readonly compressedSize: number | undefined;
}

/** Simple "does any page contain this text" PDF search (`page.getTextContent()`, no per-match
 * highlight - matching the pages a query appears on is the whole feature). */
export interface FileViewerPdfSearchState {
  readonly query: string;
  readonly regex?: boolean;
  readonly caseSensitive?: boolean;
  readonly wholeWord?: boolean;
  /** 1-based page numbers containing `query`, in ascending order. */
  readonly matches: readonly number[];
  readonly currentMatchIndex: number | undefined;
  readonly searching: boolean;
  readonly error?: string | undefined;
}

/** Search bar state for text-mode viewing (task 0088's VS-Code-like content search). */
export interface FileViewerSearchState {
  readonly query: string;
  readonly regex: boolean;
  readonly caseSensitive: boolean;
  readonly wholeWord: boolean;
  readonly matches: readonly SearchInFileMatch[];
  readonly truncated: boolean;
  readonly currentMatchIndex: number | undefined;
  readonly searching: boolean;
  readonly error: string | undefined;
}

export type FileViewerState =
  | { readonly status: 'loading'; readonly entry: EntrySummary }
  | { readonly status: 'unsupported'; readonly entry: EntrySummary }
  | { readonly status: 'error'; readonly entry: EntrySummary; readonly message: string }
  | {
      readonly status: 'ready';
      readonly entry: EntrySummary;
      readonly content:
        | FileViewerTextContent
        | FileViewerImageContent
        | FileViewerAudioContent
        | FileViewerVideoContent
        | FileViewerExternalVideoContent
        | FileViewerPdfContent
        | FileViewerComicContent
        | FileViewerEpubContent
        | FileViewerDocxContent
        | FileViewerExternalDocxContent
        | FileViewerExternalPptxContent
        | FileViewerArchiveSummaryContent
        | FileViewerStructuredTableContent
        | FileViewerStructuredJsonContent
        | FileViewerStructuredFallbackContent;
      readonly search?: FileViewerSearchState;
      /** Simple PDF text search state (`pdf` content only). */
      readonly pdfSearch?: FileViewerPdfSearchState;
      /** Simple section-level EPUB text search state (`epub` content only). */
      readonly epubSearch?: FileViewerPdfSearchState;
      /** Alt+Space info sub-panel (image/text technical metadata - task 0071). Absent/`false`
       * means closed - optional so callers/tests that never touch the panel don't need to set it. */
      readonly metadataPanelOpen?: boolean;
      /** `'loading'` while EXIF/dimensions are being parsed for an image; absent for content kinds
       * with no metadata view (audio) until/unless one is added. */
      readonly metadata?: FileViewerMetadata | 'loading';
      /** The Alt+Space panel's git history section (task 0135): commits touching this file,
       * newest first. `'loading'` while the request is in flight; an empty array once resolved
       * means the file has no history to show (outside a git working tree, on a non-local
       * provider, or not yet committed) - the section is hidden in that case, not shown empty. */
      readonly gitHistory?: readonly GitLogEntry[] | 'loading';
    };

export interface FileViewerControllerOptions {
  readonly client: FileViewerClient;
  readonly entry: EntrySummary;
  /** Needed only to list a comic archive's pages via `listDirectory` - a comic opened without this
   * shows a friendly error rather than crashing. Deliberately NOT threaded through as the caller's
   * real, active `paneId`: the backend's `list()` keys live per-pane navigation/watch state by
   * `paneId` and tears down the previous request's file-watch subscription on a mismatch, so
   * reusing the real pane here would silently corrupt that pane's own directory listing. The
   * controller mints its own throwaway pane id for this one-off request instead (see `loadComic`). */
  readonly workspaceId?: string;
  readonly update: (state: FileViewerState) => void;
  /** Pre-populated search query to run as soon as text content is ready (task 0089). */
  readonly initialSearch?: {
    readonly query: string;
    readonly regex: boolean;
    readonly caseSensitive: boolean;
    readonly wholeWord: boolean;
  };
  /** Opens the Alt+Space metadata/info panel immediately once content loads, so Alt+Space works
   * even when no viewer was already open (it opens one, with the panel visible). */
  readonly initialMetadataPanelOpen?: boolean;
}

/** Cancellable operations exposed to the presentational `FileViewer` component. */
export interface FileViewerController {
  loadMore(): Promise<void>;
  loadPrevious(): Promise<void>;
  loadTextPage(pageIndex: number): Promise<void>;
  loadStructuredRows(startRow: number): Promise<void>;
  setStructuredOptions(delimiter: string, headerMode: 'auto' | 'firstRow' | 'none'): Promise<void>;
  selectStructuredSheet(sheetName: string): Promise<void>;
  toggleStructuredRowNumbers(): void;
  loadJsonWindow(offset: number): Promise<void>;
  searchStructuredRows(query: string, cursor?: number): Promise<void>;
  sortStructuredRows(column: number): Promise<void>;
  setSearchOptions(
    patch: Partial<Pick<FileViewerSearchState, 'query' | 'regex' | 'caseSensitive' | 'wholeWord'>>,
  ): void;
  runSearch(): Promise<void>;
  goToNextMatch(): Promise<void>;
  goToPreviousMatch(): Promise<void>;
  zoomIn(): void;
  zoomOut(): void;
  setZoom(zoom: number): void;
  resetZoom(): void;
  /** Copies the currently loaded text window or image to the system clipboard. No-op for audio
   * (played back, not copyable) or non-`ready` states. */
  copyContent(): Promise<void>;
  /** Opens/closes the Alt+Space metadata/info sub-panel, computing its content on first open. */
  toggleMetadataPanel(): void;
  /** Advances to the next PDF/comic page. No-op for other content kinds or at the last page. */
  nextPage(): void;
  /** Returns to the previous PDF/comic page. No-op for other content kinds or at the first page. */
  previousPage(): void;
  /** Sets the PDF search query, debounced (`pdf` content only). */
  setPdfSearchQuery(query: string): void;
  goToNextPdfMatch(): void;
  goToPreviousPdfMatch(): void;
  setEpubSearchQuery(query: string): void;
  goToNextEpubMatch(): void;
  goToPreviousEpubMatch(): void;
  goToEpubSection(sectionIndex: number, fragment?: string): void;
  followEpubLink(href: string): void;
  goToPdfPage(pageNumber: number): void;
  goToTextOffset(offset: number, length: number): void;
  dispose(): void;
}

/** Small-source sorting may materialize at most one MiB of source-backed rows in the frontend. */
export const STRUCTURED_SORT_MAX_BYTES = 1024 * 1024;

const DEFAULT_SEARCH_STATE: FileViewerSearchState = {
  query: '',
  regex: false,
  caseSensitive: false,
  wholeWord: false,
  matches: [],
  truncated: false,
  currentMatchIndex: undefined,
  searching: false,
  error: undefined,
};

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : t('viewer', 'unableToLoad');
}

function clampZoom(zoom: number): number {
  return Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, zoom));
}

const DEFAULT_PAGED_SEARCH_STATE: FileViewerPdfSearchState = {
  query: '',
  regex: false,
  caseSensitive: false,
  wholeWord: false,
  matches: [],
  currentMatchIndex: undefined,
  searching: false,
  error: undefined,
};

function nextZoomStep(zoom: number): number {
  return clampZoom((Math.floor((zoom * 100 + 0.001) / 25) * 25 + 25) / 100);
}

function previousZoomStep(zoom: number): number {
  return clampZoom((Math.ceil((zoom * 100 - 0.001) / 25) * 25 - 25) / 100);
}

function searchExpression(search: FileViewerPdfSearchState): RegExp {
  const source =
    search.regex === true ? search.query : search.query.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  return new RegExp(
    search.wholeWord === true ? `\\b(?:${source})\\b` : source,
    search.caseSensitive === true ? 'u' : 'iu',
  );
}

async function pdfOutline(
  document: PDFDocumentProxy,
): Promise<readonly FileViewerPdfOutlineItem[]> {
  if (typeof document.getOutline !== 'function') return [];
  try {
    const source = await document.getOutline();
    if (source === null) return [];
    const result: FileViewerPdfOutlineItem[] = [];
    async function append(items: typeof source, level: number): Promise<void> {
      for (const item of items) {
        const destination =
          typeof item.dest === 'string' ? await document.getDestination(item.dest) : item.dest;
        const reference = destination?.[0];
        const page =
          typeof reference === 'number'
            ? reference + 1
            : reference === undefined
              ? undefined
              : (await document.getPageIndex(reference)) + 1;
        const label = item.title.trim();
        if (page !== undefined && label !== '') result.push({ label, level, page });
        await append(item.items, level + 1);
      }
    }
    await append(source, 1);
    return result;
  } catch {
    return [];
  }
}

/** Drives a Lister-style viewer session for exactly one entry (task 0088). */
export function createFileViewerController(
  options: FileViewerControllerOptions,
): FileViewerController {
  const { client, entry } = options;
  let disposed = false;
  let activeController: AbortController | undefined;
  let current: FileViewerState = { status: 'loading', entry };
  let search: FileViewerSearchState | undefined;
  // If an initial search query was provided, pre-populate the search state so it
  // runs as soon as text content is ready.
  if (options.initialSearch) {
    search = {
      ...DEFAULT_SEARCH_STATE,
      ...options.initialSearch,
    };
  }
  let searchDebounceTimer: ReturnType<typeof setTimeout> | undefined;
  const initialMetadataOpen = options.initialMetadataPanelOpen === true;
  /** The comic's page locations in order, populated once by `loadComic`. Kept out of published
   * state since `Location[]` is controller-internal - the view only ever sees the current page's
   * already-decoded `dataUri`. */
  let comicPageLocations: readonly Location[] = [];
  /** The EPUB's chapter locations in reading order, populated once by `loadEpub`. */
  let epubChapterLocations: readonly Location[] = [];
  let epubChapterPaths: readonly string[] = [];
  let epubImageResources: ReadonlyMap<string, EpubImageResource> = new Map();
  let epubArchiveRoot: Location | undefined;
  const epubChapterTextCache = new Map<number, string>();
  /** Per-page extracted text, cached lazily by `runPdfSearch` (1-based page number -> lowercased
   * text) so repeated searches on the same document don't re-extract every page each time. */
  const pdfPageTextCache = new Map<number, string>();
  let pdfSearchDebounceTimer: ReturnType<typeof setTimeout> | undefined;
  let pdfSearchGeneration = 0;
  let epubSearchDebounceTimer: ReturnType<typeof setTimeout> | undefined;
  let structuredStatusTimer: ReturnType<typeof setTimeout> | undefined;
  let structuredSessionId: string | undefined;
  let docxSessionId: string | undefined;
  let pptxSessionId: string | undefined;
  const textWindowCache = new Map<number, FileViewerTextContent>();

  function rememberTextWindow(content: FileViewerTextContent): void {
    textWindowCache.delete(content.windowOffset);
    textWindowCache.set(content.windowOffset, content);
    while (textWindowCache.size > 4) {
      const oldest = textWindowCache.keys().next().value;
      if (oldest === undefined) break;
      textWindowCache.delete(oldest);
    }
  }

  function publish(next: FileViewerState): void {
    current = next;
    options.update(current);
  }

  function beginRequest(): AbortController {
    activeController?.abort();
    const controller = new AbortController();
    activeController = controller;
    return controller;
  }

  function isCurrent(controller: AbortController): boolean {
    return activeController === controller && !controller.signal.aborted && !disposed;
  }

  async function loadInitialText(controller: AbortController): Promise<void> {
    const chunk = await client.readFileRange(
      { location: entry.location, offset: 0, length: TEXT_WINDOW_BYTES },
      controller.signal,
    );
    if (!isCurrent(controller)) return;
    if (chunk.probablyBinary === true) {
      publish({ status: 'unsupported', entry });
      return;
    }
    const content: FileViewerTextContent = {
      kind: 'text',
      windowOffset: 0,
      windowEnd: chunk.length,
      text: new TextDecoder().decode(new Uint8Array(chunk.data)),
      atStart: true,
      atEnd: chunk.eof,
      loadingMore: false,
    };
    rememberTextWindow(content);
    publish({
      status: 'ready',
      entry,
      content,
      metadataPanelOpen: initialMetadataOpen,
      ...(search === undefined ? {} : { search }),
    });
  }

  function structuredFormat(): 'csv' | 'tsv' | 'json' | 'ndjson' | 'excel' | undefined {
    const extension = entry.extension?.toLowerCase();
    if (extension === 'jsonl') return 'json';
    if (
      extension === 'csv' ||
      extension === 'tsv' ||
      extension === 'json' ||
      extension === 'ndjson'
    ) {
      return extension;
    }
    if (extension === 'xlsx' || extension === 'xlsb' || extension === 'xls') return 'excel';
    return undefined;
  }

  function scheduleStructuredStatusPoll(): void {
    if (structuredSessionId === undefined || disposed) return;
    structuredStatusTimer = setTimeout(() => {
      const sessionId = structuredSessionId;
      if (sessionId === undefined || disposed) return;
      void client
        .getStructuredViewStatus({ sessionId })
        .then((status) => {
          if (disposed || current.status !== 'ready' || current.content.kind !== 'structuredTable')
            return;
          publish({
            ...current,
            content: {
              ...current.content,
              indexedRows:
                current.content.searchQuery === ''
                  ? status.indexedRows
                  : current.content.indexedRows,
              totalRows:
                current.content.searchQuery === ''
                  ? (status.totalRows ?? undefined)
                  : current.content.totalRows,
              sourceIndexedRows: status.indexedRows,
              sourceTotalRows: status.totalRows ?? undefined,
              indexingComplete: status.indexingComplete,
              warning: status.warning ?? current.content.warning,
            },
          });
          if (!status.indexingComplete) scheduleStructuredStatusPoll();
        })
        .catch((error: unknown) => {
          if (!disposed) publish({ status: 'error', entry, message: errorMessage(error) });
        });
    }, 500);
  }

  async function loadStructured(
    controller: AbortController,
    format: NonNullable<ReturnType<typeof structuredFormat>>,
  ): Promise<void> {
    const opened = await client.openStructuredView(
      { location: entry.location, format, headerMode: 'auto' },
      controller.signal,
    );
    if (!isCurrent(controller)) return;
    structuredSessionId = opened.sessionId;
    if (opened.kind === 'externalFallback') {
      publish({
        status: 'ready',
        entry,
        content: {
          kind: 'structuredFallback',
          message: opened.warning ?? 'Open this file in an external spreadsheet application.',
        },
      });
      return;
    }
    if (opened.kind === 'jsonText') {
      const window = await client.readStructuredJsonWindow(
        { sessionId: opened.sessionId, offset: 0, length: TEXT_WINDOW_BYTES },
        controller.signal,
      );
      if (!isCurrent(controller)) return;
      publish({
        status: 'ready',
        entry,
        content: {
          kind: 'structuredJson',
          sessionId: opened.sessionId,
          data: window.data,
          tokens: window.tokens,
          windowOffset: window.offset,
          sourceBytes: opened.sourceBytes,
          atStart: true,
          atEnd: window.eof,
          loadingWindow: false,
          warning: opened.warning ?? undefined,
        },
      });
      return;
    }
    publish({
      status: 'ready',
      entry,
      content: {
        kind: 'structuredTable',
        sessionId: opened.sessionId,
        sourceBytes: opened.sourceBytes,
        delimiter: opened.delimiter ?? (format === 'tsv' ? '\t' : ','),
        headerMode: opened.headerMode,
        headers: opened.headers,
        rows: opened.rows,
        rowStart: opened.rows[0]?.index ?? 0,
        indexedRows: opened.indexedRows,
        totalRows: opened.totalRows ?? undefined,
        sourceIndexedRows: opened.indexedRows,
        sourceTotalRows: opened.totalRows ?? undefined,
        indexingComplete: opened.indexingComplete,
        loadingRows: false,
        warning: opened.warning ?? undefined,
        sheets: opened.sheets ?? [],
        ...(opened.selectedSheet == null ? {} : { selectedSheet: opened.selectedSheet }),
        searchQuery: '',
        searchMatches: [],
        searchNextCursor: undefined,
        searching: false,
        showRowNumbers: false,
      },
    });
    if (!opened.indexingComplete) scheduleStructuredStatusPoll();
  }

  async function loadImage(controller: AbortController): Promise<void> {
    const dataUri = await readFullImageDataUri(client, entry, controller.signal);
    if (!isCurrent(controller)) return;
    publish({
      status: 'ready',
      entry,
      content: { kind: 'image', dataUri, zoom: 1, fitToContainer: true },
      metadataPanelOpen: initialMetadataOpen,
    });
  }

  async function loadAudio(controller: AbortController): Promise<void> {
    const dataUri = await readFullAudioDataUri(client, entry, controller.signal);
    if (!isCurrent(controller)) return;
    publish({
      status: 'ready',
      entry,
      metadataPanelOpen: initialMetadataOpen,
      content: { kind: 'audio', dataUri },
    });
  }

  async function loadVideo(controller: AbortController): Promise<void> {
    const extension = entry.extension?.toLowerCase();
    if (
      entry.size === undefined ||
      entry.size > VIDEO_INLINE_SIZE_LIMIT_BYTES ||
      extension === 'mkv'
    ) {
      // Match the other loaders' asynchronous first boundary so the shell can register this
      // controller before its ready state is published.
      await Promise.resolve();
      if (!isCurrent(controller)) return;
      publish({
        status: 'ready',
        entry,
        metadataPanelOpen: initialMetadataOpen,
        content: { kind: 'videoExternal' },
      });
      return;
    }
    const dataUri = await readFullVideoDataUri(
      client,
      entry,
      controller.signal,
      VIDEO_INLINE_SIZE_LIMIT_BYTES,
    );
    if (!isCurrent(controller)) return;
    if (dataUri === undefined) {
      publish({
        status: 'ready',
        entry,
        metadataPanelOpen: initialMetadataOpen,
        content: { kind: 'videoExternal' },
      });
      return;
    }
    publish({
      status: 'ready',
      entry,
      metadataPanelOpen: initialMetadataOpen,
      content: { kind: 'video', dataUri },
    });
  }

  async function loadPdf(controller: AbortController): Promise<void> {
    const bytes = await readEntireFileBytes(client, entry, controller.signal);
    if (!isCurrent(controller)) return;
    const document = await loadPdfDocument(bytes);
    if (!isCurrent(controller)) return;
    const outline = await pdfOutline(document);
    if (!isCurrent(controller)) return;
    publish({
      status: 'ready',
      entry,
      metadataPanelOpen: initialMetadataOpen,
      content: { kind: 'pdf', document, pageCount: document.numPages, currentPage: 1, outline },
    });
  }

  /** Fetches and decodes one comic page's image bytes, publishing it as the current page. */
  async function loadComicPage(controller: AbortController, pageIndex: number): Promise<void> {
    const location = comicPageLocations[pageIndex];
    if (location === undefined) return;
    const bytes = await readEntireFileBytes(client, { ...entry, location }, controller.signal);
    if (!isCurrent(controller)) return;
    const extension = location.uri.split('.').pop()?.toLowerCase();
    const mimeType = imageMimeTypeFor({
      ...entry,
      ...(extension === undefined ? {} : { extension }),
    });
    const dataUri = bytesToDataUri(bytes, mimeType);
    if (current.status !== 'ready' || current.content.kind !== 'comic') return;
    publish({
      ...current,
      content: {
        ...current.content,
        currentPage: pageIndex,
        currentPageDataUri: dataUri,
        loadingPage: false,
      },
    });
  }

  /** Lists one archive folder and recursively descends into any subfolders, collecting every
   * image entry - some CBR/CBZ archives (notably ones exported as "one folder per volume" scans)
   * wrap their pages in a single top-level directory instead of placing them at the archive root,
   * so listing only the root would find zero pages and wrongly report the comic as unsupported.
   * Depth is capped defensively; archive trees are shallow in practice (0-2 levels). */
  async function collectComicPages(
    controller: AbortController,
    location: Location,
    depth: number,
  ): Promise<EntrySummary[]> {
    if (options.workspaceId === undefined) return [];
    const snapshot = await client.listDirectory(
      {
        workspaceId: options.workspaceId,
        // A fresh, throwaway pane id - see `FileViewerControllerOptions.workspaceId`'s doc
        // comment for why this must never be the viewer's real active pane id.
        paneId: crypto.randomUUID(),
        requestId: crypto.randomUUID(),
        location,
      },
      controller.signal,
    );
    if (!isCurrent(controller)) return [];
    const images = snapshot.entries.filter((candidate) => {
      const extension = candidate.extension?.toLowerCase();
      return (
        candidate.kind === 'file' && extension !== undefined && IMAGE_EXTENSIONS.includes(extension)
      );
    });
    if (images.length > 0 || depth >= 4) return images;
    const subdirectories = snapshot.entries.filter((candidate) => candidate.kind === 'directory');
    const nested: EntrySummary[] = [];
    for (const subdirectory of subdirectories) {
      nested.push(...(await collectComicPages(controller, subdirectory.location, depth + 1)));
      if (!isCurrent(controller)) return [];
    }
    return nested;
  }

  async function loadComic(controller: AbortController): Promise<void> {
    const archiveRoot = archiveRootForEntry(entry);
    if (archiveRoot === undefined || options.workspaceId === undefined) {
      publish({ status: 'error', entry, message: t('viewer', 'comicUnavailable') });
      return;
    }
    const pageEntries = (await collectComicPages(controller, archiveRoot, 0)).sort((a, b) =>
      a.location.uri.localeCompare(b.location.uri, undefined, { numeric: true }),
    );
    if (!isCurrent(controller)) return;
    if (pageEntries.length === 0) {
      publish({ status: 'unsupported', entry });
      return;
    }
    comicPageLocations = pageEntries.map((pageEntry) => pageEntry.location);
    publish({
      status: 'ready',
      entry,
      metadataPanelOpen: initialMetadataOpen,
      content: {
        kind: 'comic',
        pageCount: comicPageLocations.length,
        currentPage: 0,
        currentPageDataUri: undefined,
        loadingPage: true,
      },
    });
    await loadComicPage(controller, 0);
  }

  /** Fetches and sanitizes one EPUB chapter's XHTML, publishing it as the current chapter. */
  async function loadEpubChapter(controller: AbortController, chapterIndex: number): Promise<void> {
    const location = epubChapterLocations[chapterIndex];
    if (location === undefined) return;
    const bytes = await readEntireFileBytes(client, { ...entry, location }, controller.signal);
    if (!isCurrent(controller)) return;
    const chapterPath = epubChapterPaths[chapterIndex];
    const archiveRoot = epubArchiveRoot;
    if (chapterPath === undefined || archiveRoot === undefined) return;
    const html = await inlineEpubChapterImages(
      new TextDecoder().decode(bytes),
      chapterPath,
      epubImageResources,
      async (imagePath, mediaType) => {
        const imageBytes = await readEntireFileBytes(
          client,
          { ...entry, location: archiveEntryLocation(archiveRoot, imagePath) },
          controller.signal,
        );
        const safeBytes =
          mediaType.toLowerCase() === 'image/svg+xml'
            ? new TextEncoder().encode(sanitizeEpubSvg(new TextDecoder().decode(imageBytes)))
            : imageBytes;
        return bytesToDataUri(safeBytes, mediaType);
      },
    );
    if (!isCurrent(controller)) return;
    if (current.status !== 'ready' || current.content.kind !== 'epub') return;
    epubChapterTextCache.set(
      chapterIndex,
      new DOMParser().parseFromString(html, 'text/html').body.textContent?.toLowerCase() ?? '',
    );
    publish({
      ...current,
      content: {
        ...current.content,
        currentChapter: chapterIndex,
        currentChapterHtml: html,
        loadingChapter: false,
      },
    });
  }

  async function loadEpub(controller: AbortController): Promise<void> {
    const archiveRoot = archiveRootForEntry(entry);
    if (archiveRoot === undefined) {
      publish({ status: 'error', entry, message: t('viewer', 'epubUnavailable') });
      return;
    }

    const containerBytes = await readEntireFileBytes(
      client,
      { ...entry, location: archiveEntryLocation(archiveRoot, 'META-INF/container.xml') },
      controller.signal,
    );
    if (!isCurrent(controller)) return;
    const opfPath = parseEpubContainer(new TextDecoder().decode(containerBytes));
    if (opfPath === undefined) {
      publish({ status: 'error', entry, message: t('viewer', 'epubPackageMissing') });
      return;
    }
    const opfBytes = await readEntireFileBytes(
      client,
      { ...entry, location: archiveEntryLocation(archiveRoot, opfPath) },
      controller.signal,
    );
    if (!isCurrent(controller)) return;
    const book = parseEpubPackage(new TextDecoder().decode(opfBytes), opfPath);
    if (book.chapterPaths.length === 0) {
      publish({ status: 'unsupported', entry });
      return;
    }
    let chapterPaths = book.chapterPaths;
    let sectionLabelsByPath: ReadonlyMap<string, string> = new Map();
    let outline: readonly FileViewerEpubOutlineItem[] = [];
    if (book.navigationPath !== undefined) {
      const navigationBytes = await readEntireFileBytes(
        client,
        { ...entry, location: archiveEntryLocation(archiveRoot, book.navigationPath) },
        controller.signal,
      );
      if (!isCurrent(controller)) return;
      const navigationXml = new TextDecoder().decode(navigationBytes);
      chapterPaths = repairEpubChapterOrder(chapterPaths, navigationXml, book.navigationPath);
      sectionLabelsByPath = parseEpubSectionLabels(navigationXml, book.navigationPath);
      outline = parseEpubNavigationEntries(navigationXml, book.navigationPath).flatMap((item) => {
        const chapterIndex = chapterPaths.indexOf(item.path);
        if (chapterIndex === -1) return [];
        return [{ ...item, chapterIndex }];
      });
    }
    epubArchiveRoot = archiveRoot;
    epubChapterPaths = chapterPaths;
    epubImageResources = book.imageResources;
    epubChapterLocations = chapterPaths.map((path) => archiveEntryLocation(archiveRoot, path));
    publish({
      status: 'ready',
      entry,
      metadataPanelOpen: initialMetadataOpen,
      content: {
        kind: 'epub',
        title: book.title,
        chapterCount: epubChapterLocations.length,
        currentChapter: 0,
        currentChapterHtml: undefined,
        loadingChapter: true,
        zoom: 1,
        sectionLabels: chapterPaths.map((path) => sectionLabelsByPath.get(path)),
        outline,
      },
    });
    await loadEpubChapter(controller, 0);
  }

  async function loadDocx(controller: AbortController): Promise<void> {
    try {
      const opened = await client.openDocxPreview({ location: entry.location }, controller.signal);
      if (!isCurrent(controller)) {
        void client.closeDocxPreview({ sessionId: opened.sessionId }).catch(() => undefined);
        return;
      }
      docxSessionId = opened.sessionId;
      const html = await prepareDocxPreviewHtml(opened.html, opened.resources, (resourceId) =>
        client.readDocxPreviewResource(
          { sessionId: opened.sessionId, resourceId },
          controller.signal,
        ),
      );
      if (!isCurrent(controller)) return;
      const plainText =
        new DOMParser().parseFromString(html, 'text/html').body.textContent?.trim() ?? '';
      publish({
        status: 'ready',
        entry,
        metadataPanelOpen: initialMetadataOpen,
        content: {
          kind: 'docx',
          sessionId: opened.sessionId,
          sourceHtml: html,
          html,
          plainText,
          omittedFeatures: opened.omittedFeatures,
        },
      });
    } catch (error: unknown) {
      if (!isCurrent(controller)) return;
      publish({
        status: 'ready',
        entry,
        metadataPanelOpen: initialMetadataOpen,
        content: { kind: 'docxExternal', message: errorMessage(error) },
      });
    }
  }

  function failPptxPreview(controller: AbortController, error: unknown): void {
    if (!isCurrent(controller)) return;
    if (pptxSessionId !== undefined) {
      void client.closePptxPreview({ sessionId: pptxSessionId }).catch(() => undefined);
      pptxSessionId = undefined;
    }
    publish({
      status: 'ready',
      entry,
      metadataPanelOpen: initialMetadataOpen,
      content: { kind: 'pptxExternal', message: errorMessage(error) },
    });
  }

  async function loadPptx(controller: AbortController): Promise<void> {
    try {
      const opened = await client.openPptxPreview({ location: entry.location }, controller.signal);
      if (!isCurrent(controller)) {
        void client.closePptxPreview({ sessionId: opened.sessionId }).catch(() => undefined);
        return;
      }
      pptxSessionId = opened.sessionId;
      const firstPageDocument = await loadPdfDocument(new Uint8Array(opened.firstPagePdf));
      if (!isCurrent(controller)) return;
      publish({
        status: 'ready',
        entry,
        metadataPanelOpen: initialMetadataOpen,
        content: {
          kind: 'pdf',
          document: firstPageDocument,
          pageCount: firstPageDocument.numPages,
          currentPage: 1,
        },
      });

      const chunks: number[][] = [];
      let offset = 0;
      for (;;) {
        const chunk = await client.readPptxPreviewPdf(
          {
            sessionId: opened.sessionId,
            offset,
            length: PPTX_PDF_RANGE_BYTES,
          },
          controller.signal,
        );
        if (!isCurrent(controller)) return;
        if (chunk.offset !== offset || chunk.length !== chunk.data.length || chunk.length === 0) {
          throw new Error(`Invalid PowerPoint PDF range response at byte ${offset}`);
        }
        chunks.push(chunk.data);
        offset += chunk.length;
        if (chunk.eof) break;
      }
      const bytes = new Uint8Array(offset);
      let writeOffset = 0;
      for (const chunk of chunks) {
        bytes.set(chunk, writeOffset);
        writeOffset += chunk.length;
      }
      const document = await loadPdfDocument(bytes);
      if (!isCurrent(controller)) return;
      publish({
        status: 'ready',
        entry,
        metadataPanelOpen: initialMetadataOpen,
        content: { kind: 'pdf', document, pageCount: document.numPages, currentPage: 1 },
      });
      void firstPageDocument.cleanup();
    } catch (error: unknown) {
      failPptxPreview(controller, error);
    }
  }

  async function loadArchiveSummary(controller: AbortController): Promise<void> {
    if (archiveRootForEntry(entry) === undefined) {
      publish({ status: 'unsupported', entry });
      return;
    }
    const summary = await client.archiveSummary({ location: entry.location }, controller.signal);
    if (!isCurrent(controller)) return;
    publish({
      status: 'ready',
      entry,
      content: {
        kind: 'archiveSummary',
        format: summary.format,
        fileCount: summary.fileCount,
        directoryCount: summary.directoryCount,
        uncompressedSize: summary.uncompressedSize,
        compressedSize: summary.compressedSize ?? undefined,
      },
    });
  }

  async function load(): Promise<void> {
    const controller = beginRequest();
    publish({ status: 'loading', entry });
    try {
      const format = structuredFormat();
      const kind = resolvePreviewKind(entry);
      if (format !== undefined) {
        await loadStructured(controller, format);
      } else if (kind === 'image') {
        await loadImage(controller);
      } else if (kind === 'audio') {
        await loadAudio(controller);
      } else if (kind === 'video') {
        await loadVideo(controller);
      } else if (kind === 'pdf') {
        await loadPdf(controller);
      } else if (kind === 'comic') {
        await loadComic(controller);
      } else if (kind === 'epub') {
        await loadEpub(controller);
      } else if (kind === 'docx') {
        await loadDocx(controller);
      } else if (kind === 'pptx') {
        await loadPptx(controller);
      } else if (kind === 'archiveSummary') {
        await loadArchiveSummary(controller);
      } else if (kind === 'text') {
        await loadInitialText(controller);
        // Run initial search if pre-populated from content search results.
        if (search?.query.trim()) {
          await runSearch();
        }
      } else {
        publish({ status: 'unsupported', entry });
      }
      if (initialMetadataOpen && current.status === 'ready') {
        void computeMetadata();
        void computeGitHistory();
      }
    } catch (error: unknown) {
      if (isCurrent(controller)) {
        publish({ status: 'error', entry, message: errorMessage(error) });
      }
    }
  }

  function textContent(): FileViewerTextContent | undefined {
    return current.status === 'ready' && current.content.kind === 'text'
      ? current.content
      : undefined;
  }

  function imageContent(): FileViewerImageContent | undefined {
    return current.status === 'ready' && current.content.kind === 'image'
      ? current.content
      : undefined;
  }

  async function loadMore(): Promise<void> {
    const content = textContent();
    if (content === undefined || content.atEnd || content.loadingMore) return;
    const readyState = current as Extract<FileViewerState, { status: 'ready' }>;
    publish({ ...readyState, content: { ...content, loadingMore: true } });
    const controller = beginRequest();
    try {
      const nextOffset = content.windowEnd;
      const cached = textWindowCache.get(nextOffset);
      const chunk =
        cached === undefined
          ? await client.readFileRange(
              { location: entry.location, offset: nextOffset, length: TEXT_WINDOW_BYTES },
              controller.signal,
            )
          : undefined;
      if (!isCurrent(controller)) return;
      const next: FileViewerTextContent = cached ?? {
        kind: 'text',
        windowOffset: nextOffset,
        windowEnd: nextOffset + (chunk?.length ?? 0),
        text: new TextDecoder().decode(new Uint8Array(chunk?.data ?? [])),
        atStart: nextOffset === 0,
        atEnd: chunk?.eof ?? true,
        loadingMore: false,
      };
      rememberTextWindow(next);
      publish({ ...(current as Extract<FileViewerState, { status: 'ready' }>), content: next });
    } catch (error: unknown) {
      if (isCurrent(controller)) {
        publish({ status: 'error', entry, message: errorMessage(error) });
      }
    }
  }

  async function loadPrevious(): Promise<void> {
    const content = textContent();
    if (content === undefined || content.atStart || content.loadingMore) return;
    const offset = Math.max(0, content.windowOffset - TEXT_WINDOW_BYTES);
    const cached = textWindowCache.get(offset);
    if (cached !== undefined) {
      publish({ ...(current as Extract<FileViewerState, { status: 'ready' }>), content: cached });
      return;
    }

    const ready = current as Extract<FileViewerState, { status: 'ready' }>;
    publish({ ...ready, content: { ...content, loadingMore: true } });
    const controller = beginRequest();
    try {
      const chunk = await client.readFileRange(
        { location: entry.location, offset, length: content.windowOffset - offset },
        controller.signal,
      );
      if (!isCurrent(controller)) return;
      const previous: FileViewerTextContent = {
        kind: 'text',
        windowOffset: offset,
        windowEnd: offset + chunk.length,
        text: new TextDecoder().decode(new Uint8Array(chunk.data)),
        atStart: offset === 0,
        atEnd: false,
        loadingMore: false,
      };
      rememberTextWindow(previous);
      publish({ ...(current as Extract<FileViewerState, { status: 'ready' }>), content: previous });
    } catch (error: unknown) {
      if (isCurrent(controller)) publish({ status: 'error', entry, message: errorMessage(error) });
    }
  }

  async function loadTextPage(pageIndex: number): Promise<void> {
    const content = textContent();
    if (content === undefined || content.loadingMore) return;
    const offset = Math.max(0, Math.floor(pageIndex)) * TEXT_WINDOW_BYTES;
    if (offset === content.windowOffset || (entry.size !== undefined && offset >= entry.size))
      return;
    const cached = textWindowCache.get(offset);
    if (cached !== undefined) {
      publish({ ...(current as Extract<FileViewerState, { status: 'ready' }>), content: cached });
      return;
    }
    const ready = current as Extract<FileViewerState, { status: 'ready' }>;
    publish({ ...ready, content: { ...content, loadingMore: true } });
    const controller = beginRequest();
    try {
      const chunk = await client.readFileRange(
        { location: entry.location, offset, length: TEXT_WINDOW_BYTES },
        controller.signal,
      );
      if (!isCurrent(controller)) return;
      const page: FileViewerTextContent = {
        kind: 'text',
        windowOffset: offset,
        windowEnd: offset + chunk.length,
        text: new TextDecoder().decode(new Uint8Array(chunk.data)),
        atStart: offset === 0,
        atEnd: chunk.eof,
        loadingMore: false,
      };
      rememberTextWindow(page);
      publish({ ...(current as Extract<FileViewerState, { status: 'ready' }>), content: page });
    } catch (error: unknown) {
      if (isCurrent(controller)) publish({ status: 'error', entry, message: errorMessage(error) });
    }
  }

  async function loadStructuredRows(startRow: number): Promise<void> {
    if (
      current.status !== 'ready' ||
      current.content.kind !== 'structuredTable' ||
      current.content.loadingRows
    )
      return;
    const table = current.content;
    const boundedStart = Math.max(0, Math.min(startRow, Math.max(0, table.indexedRows - 1)));
    publish({ ...current, content: { ...table, loadingRows: true } });
    const controller = beginRequest();
    try {
      const result = await client.readStructuredRows(
        { sessionId: table.sessionId, startRow: boundedStart, count: 200 },
        controller.signal,
      );
      if (
        !isCurrent(controller) ||
        current.status !== 'ready' ||
        current.content.kind !== 'structuredTable'
      )
        return;
      const {
        sortColumn: _sortColumn,
        sortDirection: _sortDirection,
        ...content
      } = current.content;
      publish({
        ...current,
        content: {
          ...content,
          rows: result.rows,
          rowStart: result.rows[0]?.index ?? boundedStart,
          indexedRows: result.indexedRows,
          totalRows: result.totalRows ?? undefined,
          sourceIndexedRows: result.indexedRows,
          sourceTotalRows: result.totalRows ?? undefined,
          indexingComplete: result.indexingComplete,
          loadingRows: false,
        },
      });
    } catch (error: unknown) {
      if (isCurrent(controller)) publish({ status: 'error', entry, message: errorMessage(error) });
    }
  }

  async function setStructuredOptions(
    delimiter: string,
    headerMode: 'auto' | 'firstRow' | 'none',
  ): Promise<void> {
    if (current.status !== 'ready' || current.content.kind !== 'structuredTable') return;
    const sessionId = current.content.sessionId;
    const controller = beginRequest();
    try {
      const updated = await client.updateStructuredView(
        { sessionId, delimiter, headerMode },
        controller.signal,
      );
      if (
        !isCurrent(controller) ||
        current.status !== 'ready' ||
        current.content.kind !== 'structuredTable'
      )
        return;
      publish({
        ...current,
        content: {
          ...current.content,
          delimiter: updated.delimiter ?? delimiter,
          headerMode: updated.headerMode,
          headers: updated.headers,
          rows: updated.rows,
          rowStart: updated.rows[0]?.index ?? 0,
          indexedRows: updated.indexedRows,
          totalRows: updated.totalRows ?? undefined,
          sourceIndexedRows: updated.indexedRows,
          sourceTotalRows: updated.totalRows ?? undefined,
          indexingComplete: updated.indexingComplete,
          loadingRows: false,
        },
      });
    } catch (error: unknown) {
      if (isCurrent(controller)) publish({ status: 'error', entry, message: errorMessage(error) });
    }
  }

  async function selectStructuredSheet(sheetName: string): Promise<void> {
    if (
      current.status !== 'ready' ||
      current.content.kind !== 'structuredTable' ||
      (current.content.sheets?.length ?? 0) === 0 ||
      current.content.selectedSheet === sheetName
    )
      return;
    const sessionId = current.content.sessionId;
    const controller = beginRequest();
    try {
      const updated = await client.updateStructuredView(
        { sessionId, selectedSheet: sheetName },
        controller.signal,
      );
      if (
        !isCurrent(controller) ||
        current.status !== 'ready' ||
        current.content.kind !== 'structuredTable'
      )
        return;
      publish({
        ...current,
        content: {
          ...current.content,
          headers: updated.headers,
          rows: updated.rows,
          rowStart: updated.rows[0]?.index ?? 0,
          indexedRows: updated.indexedRows,
          totalRows: updated.totalRows ?? undefined,
          sourceIndexedRows: updated.indexedRows,
          sourceTotalRows: updated.totalRows ?? undefined,
          indexingComplete: updated.indexingComplete,
          loadingRows: false,
          warning: updated.warning ?? undefined,
          sheets: updated.sheets ?? current.content.sheets ?? [],
          selectedSheet: updated.selectedSheet ?? sheetName,
          searchQuery: '',
          searchMatches: [],
          searchNextCursor: undefined,
        },
      });
    } catch (error: unknown) {
      if (isCurrent(controller)) publish({ status: 'error', entry, message: errorMessage(error) });
    }
  }

  function toggleStructuredRowNumbers(): void {
    if (current.status !== 'ready' || current.content.kind !== 'structuredTable') return;
    publish({
      ...current,
      content: {
        ...current.content,
        showRowNumbers: current.content.showRowNumbers !== true,
      },
    });
  }

  async function loadJsonWindow(offset: number): Promise<void> {
    if (
      current.status !== 'ready' ||
      current.content.kind !== 'structuredJson' ||
      current.content.loadingWindow
    )
      return;
    const json = current.content;
    const boundedOffset = Math.max(
      0,
      Math.min(offset, Math.max(0, json.sourceBytes - TEXT_WINDOW_BYTES)),
    );
    publish({ ...current, content: { ...json, loadingWindow: true } });
    const controller = beginRequest();
    try {
      const window = await client.readStructuredJsonWindow(
        { sessionId: json.sessionId, offset: boundedOffset, length: TEXT_WINDOW_BYTES },
        controller.signal,
      );
      if (
        !isCurrent(controller) ||
        current.status !== 'ready' ||
        current.content.kind !== 'structuredJson'
      )
        return;
      publish({
        ...current,
        content: {
          ...current.content,
          data: window.data,
          tokens: window.tokens,
          windowOffset: window.offset,
          atStart: window.offset === 0,
          atEnd: window.eof,
          loadingWindow: false,
        },
      });
    } catch (error: unknown) {
      if (isCurrent(controller)) publish({ status: 'error', entry, message: errorMessage(error) });
    }
  }

  async function searchStructuredRows(query: string, cursor = 0): Promise<void> {
    if (current.status !== 'ready' || current.content.kind !== 'structuredTable') return;
    const table = current.content;
    if (query.trim() === '') {
      publish({
        ...current,
        content: {
          ...table,
          searchQuery: '',
          searchMatches: [],
          searchNextCursor: undefined,
          searching: false,
          indexedRows: table.sourceIndexedRows,
          totalRows: table.sourceTotalRows,
        },
      });
      await loadStructuredRows(0);
      return;
    }
    publish({ ...current, content: { ...table, searchQuery: query, searching: true } });
    const controller = beginRequest();
    try {
      const result = await client.searchStructuredRows(
        { sessionId: table.sessionId, query, cursor, limit: 50 },
        controller.signal,
      );
      if (
        !isCurrent(controller) ||
        current.status !== 'ready' ||
        current.content.kind !== 'structuredTable'
      )
        return;
      publish({
        ...current,
        content: {
          ...current.content,
          searchQuery: query,
          searchMatches: result.matches,
          searchNextCursor: result.nextCursor ?? undefined,
          rows: result.matches.map((row, index) => ({ ...row, index })),
          rowStart: 0,
          indexedRows: result.matches.length,
          totalRows: undefined,
          searching: false,
        },
      });
    } catch (error: unknown) {
      if (isCurrent(controller)) publish({ status: 'error', entry, message: errorMessage(error) });
    }
  }

  async function sortStructuredRows(column: number): Promise<void> {
    if (current.status !== 'ready' || current.content.kind !== 'structuredTable') return;
    const table = current.content;
    if (
      table.searchQuery === '' &&
      (!table.indexingComplete || table.sourceBytes > STRUCTURED_SORT_MAX_BYTES)
    )
      return;

    const direction =
      table.sortColumn !== column
        ? 'ascending'
        : table.sortDirection === 'ascending'
          ? 'descending'
          : table.sortDirection === 'descending'
            ? undefined
            : 'ascending';
    let rows =
      direction === undefined && table.searchQuery !== ''
        ? [...table.searchMatches]
        : [...table.rows];
    if (
      table.searchQuery === '' &&
      (direction === undefined ||
        rows.length !== (table.sourceTotalRows ?? table.sourceIndexedRows) ||
        table.rowStart !== 0)
    ) {
      const count = table.sourceTotalRows ?? table.sourceIndexedRows;
      rows = [];
      const controller = beginRequest();
      for (let startRow = 0; startRow < count; startRow += 500) {
        const page = await client.readStructuredRows(
          {
            sessionId: table.sessionId,
            startRow,
            count: Math.min(500, count - startRow),
          },
          controller.signal,
        );
        if (!isCurrent(controller)) return;
        rows.push(...page.rows);
      }
    }
    if (direction === undefined) {
      if (current.status !== 'ready' || current.content.kind !== 'structuredTable') return;
      const {
        sortColumn: _sortColumn,
        sortDirection: _sortDirection,
        ...unsortedContent
      } = current.content;
      publish({
        ...current,
        content: {
          ...unsortedContent,
          rows: rows.map((row, index) => ({ ...row, index })),
          rowStart: 0,
          indexedRows:
            table.searchQuery === '' ? table.sourceIndexedRows : table.searchMatches.length,
          totalRows: table.searchQuery === '' ? table.sourceTotalRows : undefined,
        },
      });
      return;
    }
    rows.sort((left, right) => {
      const leftValue = left.cells[column] ?? '';
      const rightValue = right.cells[column] ?? '';
      if (leftValue === '') return rightValue === '' ? 0 : 1;
      if (rightValue === '') return -1;
      const comparison = leftValue.localeCompare(rightValue, undefined, {
        numeric: true,
        sensitivity: 'base',
      });
      return direction === 'ascending' ? comparison : -comparison;
    });
    if (current.status !== 'ready' || current.content.kind !== 'structuredTable') return;
    publish({
      ...current,
      content: {
        ...current.content,
        rows: rows.map((row, index) => ({ ...row, index })),
        rowStart: 0,
        indexedRows: rows.length,
        totalRows: rows.length,
        sortColumn: column,
        sortDirection: direction,
      },
    });
  }

  function setSearchOptions(
    patch: Partial<Pick<FileViewerSearchState, 'query' | 'regex' | 'caseSensitive' | 'wholeWord'>>,
  ): void {
    if (current.status === 'ready' && current.content.kind === 'pdf') {
      const next = { ...(current.pdfSearch ?? DEFAULT_PAGED_SEARCH_STATE), ...patch };
      publish({ ...current, pdfSearch: next });
      setPdfSearchQuery(next.query);
      return;
    }
    if (current.status === 'ready' && current.content.kind === 'epub') {
      const next = { ...(current.epubSearch ?? DEFAULT_PAGED_SEARCH_STATE), ...patch };
      publish({ ...current, epubSearch: next });
      setEpubSearchQuery(next.query);
      return;
    }
    search = { ...(search ?? DEFAULT_SEARCH_STATE), ...patch };
    if (current.status === 'ready') {
      publish({ ...current, search });
    }
    if (searchDebounceTimer !== undefined) {
      clearTimeout(searchDebounceTimer);
      searchDebounceTimer = undefined;
    }
    if (search.query.trim() === '') {
      // Nothing to search - clear stale results and highlight immediately rather than waiting on the debounce.
      search = {
        ...search,
        matches: [],
        truncated: false,
        currentMatchIndex: undefined,
        searching: false,
        error: undefined,
      };
      if (current.status === 'ready') {
        const readyState = current as Extract<FileViewerState, { status: 'ready' }>;
        // Also clear any stale highlight from the content state.
        if (
          readyState.content.kind === 'text' &&
          (readyState.content.highlightOffset !== undefined ||
            readyState.content.highlightLength !== undefined)
        ) {
          const { highlightOffset, highlightLength, ...contentRest } = readyState.content;
          publish({
            ...readyState,
            content: contentRest,
            search,
          });
        } else if (readyState.content.kind === 'docx') {
          publish({
            ...readyState,
            content: { ...readyState.content, html: readyState.content.sourceHtml },
            search,
          });
        } else {
          publish({ ...current, search });
        }
      }
      return;
    }
    searchDebounceTimer = setTimeout(() => {
      searchDebounceTimer = undefined;
      void runSearch();
    }, SEARCH_DEBOUNCE_MS);
  }

  async function jumpToMatch(index: number): Promise<void> {
    const match = search?.matches[index];
    if (match === undefined) return;
    if (current.status === 'ready' && current.content.kind === 'docx' && search !== undefined) {
      const result = searchDocxHtml(
        current.content.sourceHtml,
        search.query,
        search.regex,
        search.caseSensitive,
        search.wholeWord,
        index,
      );
      search = { ...search, matches: result.matches, currentMatchIndex: index };
      publish({
        ...current,
        content: { ...current.content, html: result.html },
        search,
      });
      return;
    }
    const windowOffset = Math.max(0, match.offset - JUMP_CONTEXT_BEFORE_BYTES);
    const length = Math.max(TEXT_WINDOW_BYTES, match.offset + match.length - windowOffset);
    search = { ...(search ?? DEFAULT_SEARCH_STATE), currentMatchIndex: index };
    // Stay in the 'ready' status while fetching (rather than bouncing through 'loading') so the
    // search bar/input never unmounts - that would drop keyboard focus and flicker the viewer.
    if (current.status === 'ready') publish({ ...current, search });
    const controller = beginRequest();
    try {
      const chunk = await client.readFileRange(
        { location: entry.location, offset: windowOffset, length },
        controller.signal,
      );
      if (!isCurrent(controller)) return;
      const bytes = new Uint8Array(chunk.data);
      // Convert the match's byte offset/length (relative to this window) into character offsets by
      // decoding only the bytes before/within the match - see `FileViewerTextContent`'s doc comment.
      const matchStartInChunk = match.offset - windowOffset;
      const highlightOffset = new TextDecoder().decode(bytes.subarray(0, matchStartInChunk)).length;
      const highlightLength = new TextDecoder().decode(
        bytes.subarray(matchStartInChunk, matchStartInChunk + match.length),
      ).length;
      publish({
        status: 'ready',
        entry,
        content: {
          kind: 'text',
          windowOffset,
          windowEnd: windowOffset + chunk.length,
          text: new TextDecoder().decode(bytes),
          atStart: windowOffset === 0,
          atEnd: chunk.eof,
          loadingMore: false,
          highlightOffset,
          highlightLength,
        },
        metadataPanelOpen: current.status === 'ready' && (current.metadataPanelOpen ?? false),
        ...(search === undefined ? {} : { search }),
      });
      if (current.status === 'ready' && current.metadataPanelOpen === true) void computeMetadata();
    } catch (error: unknown) {
      if (isCurrent(controller)) {
        publish({ status: 'error', entry, message: errorMessage(error) });
      }
    }
  }

  async function runSearch(): Promise<void> {
    if (searchDebounceTimer !== undefined) {
      clearTimeout(searchDebounceTimer);
      searchDebounceTimer = undefined;
    }
    const options_ = search ?? DEFAULT_SEARCH_STATE;
    if (options_.query.trim() === '') return;
    search = { ...options_, searching: true, error: undefined };
    if (current.status === 'ready') publish({ ...current, search });
    const controller = beginRequest();
    try {
      if (current.status === 'ready' && current.content.kind === 'docx') {
        const result = searchDocxHtml(
          current.content.sourceHtml,
          options_.query,
          options_.regex,
          options_.caseSensitive,
          options_.wholeWord,
          0,
        );
        if (!isCurrent(controller)) return;
        search = {
          ...options_,
          matches: result.matches,
          truncated: result.truncated,
          currentMatchIndex: result.matches.length > 0 ? 0 : undefined,
          searching: false,
        };
        publish({
          ...current,
          content: { ...current.content, html: result.html },
          search,
        });
        return;
      }
      const result = await client.searchInFile(
        {
          location: entry.location,
          query: options_.query,
          regex: options_.regex,
          caseSensitive: options_.caseSensitive,
          wholeWord: options_.wholeWord,
        },
        controller.signal,
      );
      if (!isCurrent(controller)) return;
      search = {
        ...options_,
        matches: result.matches,
        truncated: result.truncated,
        currentMatchIndex: undefined,
        searching: false,
      };
      if (current.status === 'ready') publish({ ...current, search });
      if (result.matches.length > 0) {
        await jumpToMatch(0);
      } else {
        // No matches - clear stale highlight from the content state.
        const content = textContent();
        if (
          content !== undefined &&
          (content.highlightOffset !== undefined || content.highlightLength !== undefined)
        ) {
          const { highlightOffset, highlightLength, ...contentRest } = content;
          publish({
            ...(current as Extract<FileViewerState, { status: 'ready' }>),
            content: contentRest,
          });
        }
      }
    } catch (error: unknown) {
      if (!isCurrent(controller)) return;
      search = { ...options_, searching: false, error: errorMessage(error) };
      if (current.status === 'ready') publish({ ...current, search });
    }
  }

  async function goToNextMatch(): Promise<void> {
    if (search === undefined || search.matches.length === 0) return;
    const next = ((search.currentMatchIndex ?? -1) + 1) % search.matches.length;
    await jumpToMatch(next);
  }

  async function goToPreviousMatch(): Promise<void> {
    if (search === undefined || search.matches.length === 0) return;
    const count = search.matches.length;
    const previous = ((search.currentMatchIndex ?? 0) - 1 + count) % count;
    await jumpToMatch(previous);
  }

  function zoomIn(): void {
    if (current.status === 'ready' && current.content.kind === 'epub') {
      publish({
        ...current,
        content: { ...current.content, zoom: nextZoomStep(current.content.zoom) },
      });
      return;
    }
    const content = imageContent();
    if (content === undefined) return;
    const base = content.fitToContainer ? 1 : content.zoom;
    publish({
      ...(current as Extract<FileViewerState, { status: 'ready' }>),
      content: { ...content, zoom: nextZoomStep(base), fitToContainer: false },
    });
  }

  function zoomOut(): void {
    if (current.status === 'ready' && current.content.kind === 'epub') {
      publish({
        ...current,
        content: { ...current.content, zoom: previousZoomStep(current.content.zoom) },
      });
      return;
    }
    const content = imageContent();
    if (content === undefined) return;
    const base = content.fitToContainer ? 1 : content.zoom;
    publish({
      ...(current as Extract<FileViewerState, { status: 'ready' }>),
      content: { ...content, zoom: previousZoomStep(base), fitToContainer: false },
    });
  }

  function setZoom(zoom: number): void {
    if (!Number.isFinite(zoom)) return;
    if (current.status === 'ready' && current.content.kind === 'epub') {
      publish({ ...current, content: { ...current.content, zoom: clampZoom(zoom) } });
      return;
    }
    const content = imageContent();
    if (content === undefined) return;
    publish({
      ...(current as Extract<FileViewerState, { status: 'ready' }>),
      content: { ...content, zoom: clampZoom(zoom), fitToContainer: false },
    });
  }

  function resetZoom(): void {
    if (current.status === 'ready' && current.content.kind === 'epub') {
      publish({ ...current, content: { ...current.content, zoom: 1 } });
      return;
    }
    const content = imageContent();
    if (content === undefined) return;
    publish({
      ...(current as Extract<FileViewerState, { status: 'ready' }>),
      content: { ...content, zoom: 1, fitToContainer: true },
    });
  }

  async function copyContent(): Promise<void> {
    if (current.status !== 'ready') return;
    if (current.content.kind === 'text') {
      await copyText(current.content.text);
    } else if (current.content.kind === 'docx') {
      await copyText(current.content.plainText);
    } else if (current.content.kind === 'image') {
      await copyImageDataUri(current.content.dataUri);
    }
  }

  /** Computes (or recomputes) the info panel's metadata for the currently loaded content. Marks
   * the state `metadata: 'loading'` immediately for image content (EXIF parsing is async);
   * text metadata is derived synchronously from the already-loaded window. */
  async function computeMetadata(): Promise<void> {
    if (current.status !== 'ready') return;
    const ready = current as Extract<FileViewerState, { status: 'ready' }>;
    if (ready.content.kind === 'text') {
      publish({
        ...ready,
        metadata: textMetadataFor(
          entry,
          ready.content.text,
          !ready.content.atStart || !ready.content.atEnd,
          editableLanguageForExtension(entry.extension, entry.name),
        ),
      });
    } else if (ready.content.kind === 'docx') {
      publish({
        ...ready,
        metadata: textMetadataFor(entry, ready.content.plainText, false, 'text'),
      });
      return;
    }
    if (ready.content.kind !== 'image') return;
    const dataUri = ready.content.dataUri;
    publish({ ...ready, metadata: 'loading' });
    const [dimensions, exif] = await Promise.all([
      readImageDimensions(dataUri),
      readImageExif(dataUri),
    ]);
    if (current.status !== 'ready' || current.content.kind !== 'image') return;
    publish({
      ...(current as Extract<FileViewerState, { status: 'ready' }>),
      metadata: {
        kind: 'image',
        width: dimensions?.width,
        height: dimensions?.height,
        sizeBytes: entry.size,
        mimeType: imageMimeTypeFor(entry),
        ...exif,
      },
    });
  }

  /** Fetches the info panel's git history section (task 0135). Never throws to the caller - a
   * failed request (e.g. an unreachable backend) resolves to no history, same as a file that
   * genuinely has none, since there is nothing actionable a user could do with a git-history
   * fetch error here. */
  async function computeGitHistory(): Promise<void> {
    if (current.status !== 'ready') return;
    const ready = current as Extract<FileViewerState, { status: 'ready' }>;
    publish({ ...ready, gitHistory: 'loading' });
    let commits: readonly GitLogEntry[] = [];
    try {
      commits = (await client.gitFileHistory({ location: entry.location })).commits;
    } catch {
      commits = [];
    }
    if (current.status !== 'ready') return;
    publish({ ...(current as Extract<FileViewerState, { status: 'ready' }>), gitHistory: commits });
  }

  function toggleMetadataPanel(): void {
    if (current.status !== 'ready') return;
    const ready = current as Extract<FileViewerState, { status: 'ready' }>;
    const open = !(ready.metadataPanelOpen ?? false);
    publish({ ...ready, metadataPanelOpen: open });
    if (open && ready.metadata === undefined) void computeMetadata();
    if (open && ready.gitHistory === undefined) void computeGitHistory();
  }

  function nextPage(): void {
    if (current.status !== 'ready') return;
    if (current.content.kind === 'pdf') {
      if (current.content.currentPage >= current.content.pageCount) return;
      publish({
        ...current,
        content: { ...current.content, currentPage: current.content.currentPage + 1 },
      });
    } else if (current.content.kind === 'comic') {
      const nextIndex = current.content.currentPage + 1;
      if (nextIndex >= current.content.pageCount) return;
      publish({
        ...current,
        content: {
          ...current.content,
          currentPage: nextIndex,
          currentPageDataUri: undefined,
          loadingPage: true,
        },
      });
      void loadComicPage(beginRequest(), nextIndex);
    } else if (current.content.kind === 'epub') {
      const nextIndex = current.content.currentChapter + 1;
      if (nextIndex >= current.content.chapterCount) return;
      const { targetFragment: _targetFragment, ...content } = current.content;
      publish({
        ...current,
        content: {
          ...content,
          currentChapter: nextIndex,
          currentChapterHtml: undefined,
          loadingChapter: true,
        },
      });
      void loadEpubChapter(beginRequest(), nextIndex);
    }
  }

  function previousPage(): void {
    if (current.status !== 'ready') return;
    if (current.content.kind === 'pdf') {
      if (current.content.currentPage <= 1) return;
      publish({
        ...current,
        content: { ...current.content, currentPage: current.content.currentPage - 1 },
      });
    } else if (current.content.kind === 'comic') {
      const previousIndex = current.content.currentPage - 1;
      if (previousIndex < 0) return;
      publish({
        ...current,
        content: {
          ...current.content,
          currentPage: previousIndex,
          currentPageDataUri: undefined,
          loadingPage: true,
        },
      });
      void loadComicPage(beginRequest(), previousIndex);
    } else if (current.content.kind === 'epub') {
      const previousIndex = current.content.currentChapter - 1;
      if (previousIndex < 0) return;
      const { targetFragment: _targetFragment, ...content } = current.content;
      publish({
        ...current,
        content: {
          ...content,
          currentChapter: previousIndex,
          currentChapterHtml: undefined,
          loadingChapter: true,
        },
      });
      void loadEpubChapter(beginRequest(), previousIndex);
    }
  }

  function goToEpubSection(sectionIndex: number, fragment?: string): void {
    if (
      current.status !== 'ready' ||
      current.content.kind !== 'epub' ||
      sectionIndex < 0 ||
      sectionIndex >= current.content.chapterCount
    )
      return;
    const { targetFragment: _targetFragment, ...content } = current.content;
    if (sectionIndex === current.content.currentChapter) {
      publish({
        ...current,
        content: fragment === undefined ? content : { ...content, targetFragment: fragment },
      });
      return;
    }
    publish({
      ...current,
      content: {
        ...content,
        currentChapter: sectionIndex,
        currentChapterHtml: undefined,
        loadingChapter: true,
        ...(fragment === undefined ? {} : { targetFragment: fragment }),
      },
    });
    void loadEpubChapter(beginRequest(), sectionIndex);
  }

  function followEpubLink(href: string): void {
    if (current.status !== 'ready' || current.content.kind !== 'epub') return;
    const currentPath = epubChapterPaths[current.content.currentChapter];
    if (currentPath === undefined) return;
    const hashIndex = href.indexOf('#');
    const hrefPath = (hashIndex === -1 ? href : href.slice(0, hashIndex)).split('?')[0] ?? '';
    const fragment = hashIndex === -1 ? undefined : href.slice(hashIndex + 1);
    const currentDirectory = currentPath.slice(0, currentPath.lastIndexOf('/') + 1);
    const targetPath = hrefPath === '' ? currentPath : resolveEpubPath(currentDirectory, hrefPath);
    const targetIndex = epubChapterPaths.indexOf(targetPath);
    if (targetIndex === -1) return;
    if (targetIndex === current.content.currentChapter) {
      const { targetFragment: _targetFragment, ...content } = current.content;
      publish({
        ...current,
        content: fragment === undefined ? content : { ...content, targetFragment: fragment },
      });
      return;
    }
    const { targetFragment: _targetFragment, ...content } = current.content;
    publish({
      ...current,
      content: {
        ...content,
        currentChapter: targetIndex,
        currentChapterHtml: undefined,
        loadingChapter: true,
        ...(fragment === undefined ? {} : { targetFragment: fragment }),
      },
    });
    void loadEpubChapter(beginRequest(), targetIndex);
  }

  function goToPdfPage(pageNumber: number): void {
    if (
      current.status !== 'ready' ||
      current.content.kind !== 'pdf' ||
      pageNumber < 1 ||
      pageNumber > current.content.pageCount
    )
      return;
    publish({ ...current, content: { ...current.content, currentPage: pageNumber } });
  }

  function goToTextOffset(offset: number, length: number): void {
    if (current.status !== 'ready' || current.content.kind !== 'text') return;
    if (offset < 0 || offset > current.content.text.length) return;
    publish({
      ...current,
      content: {
        ...current.content,
        highlightOffset: offset,
        highlightLength: Math.max(1, Math.min(length, current.content.text.length - offset)),
      },
    });
  }

  async function pdfPageText(document: PDFDocumentProxy, pageNumber: number): Promise<string> {
    const cached = pdfPageTextCache.get(pageNumber);
    if (cached !== undefined) return cached;
    const page = await document.getPage(pageNumber);
    const content = await page.getTextContent();
    const text = content.items.map((item) => ('str' in item ? item.str : '')).join(' ');
    pdfPageTextCache.set(pageNumber, text);
    return text;
  }

  async function runPdfSearch(generation: number): Promise<void> {
    if (current.status !== 'ready' || current.content.kind !== 'pdf') return;
    const document = current.content.document;
    const initialPage = current.content.currentPage;
    const pagedSearch = current.pdfSearch ?? DEFAULT_PAGED_SEARCH_STATE;
    const query = pagedSearch.query.trim();
    if (query === '') return;
    let expression: RegExp;
    try {
      expression = searchExpression({ ...pagedSearch, query });
    } catch (error: unknown) {
      publish({
        ...current,
        pdfSearch: {
          ...pagedSearch,
          searching: false,
          error: error instanceof Error ? error.message : String(error),
        },
      });
      return;
    }
    publish({
      ...current,
      pdfSearch: { ...pagedSearch, query, searching: true, error: undefined },
    });
    const matches: number[] = [];
    const pageOrder = [
      initialPage,
      ...Array.from({ length: document.numPages }, (_, index) => index + 1).filter(
        (pageNumber) => pageNumber !== initialPage,
      ),
    ];
    for (const pageNumber of pageOrder) {
      if (
        generation !== pdfSearchGeneration ||
        current.status !== 'ready' ||
        current.content.kind !== 'pdf'
      )
        return;
      const text = await pdfPageText(document, pageNumber);
      if (generation !== pdfSearchGeneration) return;
      if (expression.test(text)) {
        matches.push(pageNumber);
        publish({
          ...current,
          content:
            matches.length === 1
              ? { ...current.content, currentPage: pageNumber }
              : current.content,
          pdfSearch: {
            ...pagedSearch,
            query,
            matches: [...matches],
            currentMatchIndex: 0,
            searching: true,
            error: undefined,
          },
        });
      }
    }
    if (
      generation !== pdfSearchGeneration ||
      current.status !== 'ready' ||
      current.content.kind !== 'pdf'
    )
      return;
    matches.sort((left, right) => left - right);
    const currentMatchIndex = matches.indexOf(current.content.currentPage);
    publish({
      ...current,
      pdfSearch: {
        ...pagedSearch,
        query,
        matches,
        currentMatchIndex:
          matches.length === 0 ? undefined : currentMatchIndex === -1 ? 0 : currentMatchIndex,
        searching: false,
        error: undefined,
      },
    });
  }

  function setPdfSearchQuery(query: string): void {
    if (current.status !== 'ready' || current.content.kind !== 'pdf') return;
    pdfSearchGeneration += 1;
    const generation = pdfSearchGeneration;
    publish({
      ...current,
      pdfSearch: {
        ...(current.pdfSearch ?? DEFAULT_PAGED_SEARCH_STATE),
        query,
        matches: current.pdfSearch?.matches ?? [],
        currentMatchIndex: current.pdfSearch?.currentMatchIndex,
        searching: false,
        error: undefined,
      },
    });
    if (pdfSearchDebounceTimer !== undefined) clearTimeout(pdfSearchDebounceTimer);
    if (query.trim() === '') {
      publish({
        ...(current as Extract<FileViewerState, { status: 'ready' }>),
        pdfSearch: {
          ...(current.pdfSearch ?? DEFAULT_PAGED_SEARCH_STATE),
          query: '',
          matches: [],
          currentMatchIndex: undefined,
          searching: false,
          error: undefined,
        },
      });
      return;
    }
    pdfSearchDebounceTimer = setTimeout(() => {
      pdfSearchDebounceTimer = undefined;
      void runPdfSearch(generation);
    }, SEARCH_DEBOUNCE_MS);
  }

  function goToPdfMatch(index: number): void {
    if (
      current.status !== 'ready' ||
      current.content.kind !== 'pdf' ||
      current.pdfSearch === undefined
    )
      return;
    const page = current.pdfSearch.matches[index];
    if (page === undefined) return;
    publish({
      ...current,
      content: { ...current.content, currentPage: page },
      pdfSearch: { ...current.pdfSearch, currentMatchIndex: index },
    });
  }

  function goToNextPdfMatch(): void {
    if (
      current.status !== 'ready' ||
      current.pdfSearch === undefined ||
      current.pdfSearch.matches.length === 0
    )
      return;
    goToPdfMatch(
      ((current.pdfSearch.currentMatchIndex ?? -1) + 1) % current.pdfSearch.matches.length,
    );
  }

  function goToPreviousPdfMatch(): void {
    if (
      current.status !== 'ready' ||
      current.pdfSearch === undefined ||
      current.pdfSearch.matches.length === 0
    )
      return;
    const count = current.pdfSearch.matches.length;
    goToPdfMatch(((current.pdfSearch.currentMatchIndex ?? 0) - 1 + count) % count);
  }

  async function epubChapterText(chapterIndex: number, signal: AbortSignal): Promise<string> {
    const cached = epubChapterTextCache.get(chapterIndex);
    if (cached !== undefined) return cached;
    const location = epubChapterLocations[chapterIndex];
    if (location === undefined) return '';
    const bytes = await readEntireFileBytes(client, { ...entry, location }, signal);
    const text =
      new DOMParser().parseFromString(new TextDecoder().decode(bytes), 'text/html').body
        .textContent ?? '';
    epubChapterTextCache.set(chapterIndex, text);
    return text;
  }

  async function runEpubSearch(): Promise<void> {
    if (current.status !== 'ready' || current.content.kind !== 'epub') return;
    const pagedSearch = current.epubSearch ?? DEFAULT_PAGED_SEARCH_STATE;
    const query = pagedSearch.query.trim();
    if (query === '') return;
    let expression: RegExp;
    try {
      expression = searchExpression({ ...pagedSearch, query });
    } catch (error: unknown) {
      publish({
        ...current,
        epubSearch: {
          ...pagedSearch,
          searching: false,
          error: error instanceof Error ? error.message : String(error),
        },
      });
      return;
    }
    publish({
      ...current,
      epubSearch: { ...pagedSearch, query, searching: true, error: undefined },
    });
    const controller = beginRequest();
    const matches: number[] = [];
    for (let chapterIndex = 0; chapterIndex < epubChapterLocations.length; chapterIndex += 1) {
      const text = await epubChapterText(chapterIndex, controller.signal);
      if (!isCurrent(controller)) return;
      if (expression.test(text)) matches.push(chapterIndex + 1);
    }
    if (!isCurrent(controller) || current.status !== 'ready' || current.content.kind !== 'epub')
      return;
    publish({
      ...current,
      epubSearch: {
        ...pagedSearch,
        query,
        matches,
        currentMatchIndex: matches.length > 0 ? 0 : undefined,
        searching: false,
        error: undefined,
      },
    });
    if (matches.length > 0) goToEpubMatch(0);
  }

  function setEpubSearchQuery(query: string): void {
    if (current.status !== 'ready' || current.content.kind !== 'epub') return;
    publish({
      ...current,
      epubSearch: {
        ...(current.epubSearch ?? DEFAULT_PAGED_SEARCH_STATE),
        query,
        matches: current.epubSearch?.matches ?? [],
        currentMatchIndex: current.epubSearch?.currentMatchIndex,
        searching: false,
        error: undefined,
      },
    });
    if (epubSearchDebounceTimer !== undefined) clearTimeout(epubSearchDebounceTimer);
    if (query.trim() === '') {
      publish({
        ...current,
        epubSearch: {
          ...(current.epubSearch ?? DEFAULT_PAGED_SEARCH_STATE),
          query: '',
          matches: [],
          currentMatchIndex: undefined,
          searching: false,
          error: undefined,
        },
      });
      return;
    }
    epubSearchDebounceTimer = setTimeout(() => {
      epubSearchDebounceTimer = undefined;
      void runEpubSearch();
    }, SEARCH_DEBOUNCE_MS);
  }

  function goToEpubMatch(index: number): void {
    if (
      current.status !== 'ready' ||
      current.content.kind !== 'epub' ||
      current.epubSearch === undefined
    )
      return;
    const chapterNumber = current.epubSearch.matches[index];
    if (chapterNumber === undefined) return;
    const chapterIndex = chapterNumber - 1;
    const { targetFragment: _targetFragment, ...content } = current.content;
    publish({
      ...current,
      content: {
        ...content,
        currentChapter: chapterIndex,
        currentChapterHtml: undefined,
        loadingChapter: true,
      },
      epubSearch: { ...current.epubSearch, currentMatchIndex: index },
    });
    void loadEpubChapter(beginRequest(), chapterIndex);
  }

  function goToNextEpubMatch(): void {
    if (current.status !== 'ready' || current.epubSearch?.matches.length === 0) return;
    goToEpubMatch(
      ((current.epubSearch?.currentMatchIndex ?? -1) + 1) %
        (current.epubSearch?.matches.length ?? 1),
    );
  }

  function goToPreviousEpubMatch(): void {
    if (current.status !== 'ready' || current.epubSearch?.matches.length === 0) return;
    const count = current.epubSearch?.matches.length ?? 1;
    goToEpubMatch(((current.epubSearch?.currentMatchIndex ?? 0) - 1 + count) % count);
  }

  void load();

  return {
    loadMore,
    loadPrevious,
    loadTextPage,
    loadStructuredRows,
    setStructuredOptions,
    selectStructuredSheet,
    toggleStructuredRowNumbers,
    loadJsonWindow,
    searchStructuredRows,
    sortStructuredRows,
    setSearchOptions,
    runSearch,
    goToNextMatch,
    goToPreviousMatch,
    zoomIn,
    zoomOut,
    setZoom,
    resetZoom,
    copyContent,
    toggleMetadataPanel,
    nextPage,
    previousPage,
    setPdfSearchQuery,
    goToNextPdfMatch,
    goToPreviousPdfMatch,
    setEpubSearchQuery,
    goToNextEpubMatch,
    goToPreviousEpubMatch,
    goToEpubSection,
    followEpubLink,
    goToPdfPage,
    goToTextOffset,
    dispose: () => {
      disposed = true;
      pdfSearchGeneration += 1;
      activeController?.abort();
      if (searchDebounceTimer !== undefined) {
        clearTimeout(searchDebounceTimer);
        searchDebounceTimer = undefined;
      }
      if (pdfSearchDebounceTimer !== undefined) {
        clearTimeout(pdfSearchDebounceTimer);
        pdfSearchDebounceTimer = undefined;
      }
      if (epubSearchDebounceTimer !== undefined) {
        clearTimeout(epubSearchDebounceTimer);
        epubSearchDebounceTimer = undefined;
      }
      if (structuredStatusTimer !== undefined) {
        clearTimeout(structuredStatusTimer);
        structuredStatusTimer = undefined;
      }
      if (structuredSessionId !== undefined) {
        void client.closeStructuredView({ sessionId: structuredSessionId }).catch(() => undefined);
        structuredSessionId = undefined;
      }
      if (docxSessionId !== undefined) {
        void client.closeDocxPreview({ sessionId: docxSessionId }).catch(() => undefined);
        docxSessionId = undefined;
      }
      if (pptxSessionId !== undefined) {
        void client.closePptxPreview({ sessionId: pptxSessionId }).catch(() => undefined);
        pptxSessionId = undefined;
      }
    },
  };
}
