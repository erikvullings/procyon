import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { EntrySummary } from '../../models';
import { FileViewer, type FileViewerAttrs } from './file-viewer';
import type { FileViewerState } from './file-viewer-controller';

const renderPdfPageToCanvas = vi.fn().mockResolvedValue(undefined);
vi.mock('./pdf-preview', () => ({
  renderPdfPageToCanvas: (...args: unknown[]) => renderPdfPageToCanvas(...args),
}));

let root: HTMLElement;

function entry(overrides: Partial<EntrySummary> = {}): EntrySummary {
  return {
    id: 'entry-report.txt',
    location: { providerId: 'local', uri: 'file:///tmp/report.txt' },
    name: 'report.txt',
    kind: 'file',
    hidden: false,
    readOnly: false,
    metadataRevision: 1,
    ...overrides,
  };
}

function baseAttrs(
  state: FileViewerState,
  overrides: Partial<FileViewerAttrs> = {},
): FileViewerAttrs {
  return {
    state,
    onLoadMore: vi.fn(),
    onLoadPrevious: vi.fn(),
    onLoadStructuredRows: vi.fn(),
    onStructuredOptionsChange: vi.fn(),
    onSelectStructuredSheet: vi.fn(),
    onLoadJsonWindow: vi.fn(),
    onSearchStructuredRows: vi.fn(),
    onSortStructuredRows: vi.fn(),
    onSearchQueryChange: vi.fn(),
    onSearchOptionChange: vi.fn(),
    onRunSearch: vi.fn(),
    onNextMatch: vi.fn(),
    onPreviousMatch: vi.fn(),
    onZoomIn: vi.fn(),
    onZoomOut: vi.fn(),
    onResetZoom: vi.fn(),
    onCopy: vi.fn().mockResolvedValue(undefined),
    onToggleMetadata: vi.fn(),
    onNextPage: vi.fn(),
    onPreviousPage: vi.fn(),
    onPdfSearchQueryChange: vi.fn(),
    onNextPdfMatch: vi.fn(),
    onPreviousPdfMatch: vi.fn(),
    quickLookAvailable: false,
    onQuickLook: vi.fn(),
    onOpenExternally: vi.fn(),
    onClose: vi.fn(),
    ...overrides,
  };
}

function mount(attrs: FileViewerAttrs): void {
  m.mount(root, { view: () => m(FileViewer, attrs) });
}

beforeEach(() => {
  root = document.createElement('div');
  document.body.appendChild(root);
});

afterEach(() => {
  m.mount(root, null);
  root.remove();
});

describe('FileViewer', () => {
  it('renders archive format, entry totals, sizes, and compression ratio', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'bundle.zip', extension: 'zip' }),
        content: {
          kind: 'archiveSummary',
          format: 'zip',
          fileCount: 3,
          directoryCount: 2,
          uncompressedSize: 4_096,
          compressedSize: 1_024,
        },
      }),
    );

    const summary = root.querySelector('.fm-file-viewer-archive-summary');
    expect(summary?.textContent).toContain('ZIP');
    expect(summary?.textContent).toContain('3 files');
    expect(summary?.textContent).toContain('2 directories');
    expect(summary?.textContent).toContain('4 KB');
    expect(summary?.textContent).toContain('1 KB');
    expect(summary?.textContent).toContain('4:1');
  });

  it('renders unavailable compression values for an uncompressed tar archive', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'bundle.tar', extension: 'tar' }),
        content: {
          kind: 'archiveSummary',
          format: 'tar',
          fileCount: 1,
          directoryCount: 0,
          uncompressedSize: 10,
          compressedSize: undefined,
        },
      }),
    );

    expect(root.querySelector('.fm-file-viewer-archive-summary')?.textContent).toContain('N/A');
  });

  it('shows a loading message while loading', () => {
    mount(baseAttrs({ status: 'loading', entry: entry() }));
    expect(root.querySelector('.fm-file-viewer-body')?.textContent).toBe('Loading…');
  });

  it('shows an unsupported message for binary content', () => {
    mount(baseAttrs({ status: 'unsupported', entry: entry() }));
    expect(root.querySelector('.fm-file-viewer-body')?.textContent).toContain(
      'Preview not available',
    );
  });

  it('offers Quick Look and external open for unsupported local content', () => {
    const onQuickLook = vi.fn();
    const onOpenExternally = vi.fn();
    mount(
      baseAttrs(
        { status: 'unsupported', entry: entry() },
        { quickLookAvailable: true, onQuickLook, onOpenExternally },
      ),
    );

    root.querySelector<HTMLButtonElement>('.fm-file-viewer-quick-look')?.click();
    root.querySelector<HTMLButtonElement>('.fm-file-viewer-open-externally')?.click();

    expect(onQuickLook).toHaveBeenCalledOnce();
    expect(onOpenExternally).toHaveBeenCalledOnce();
  });

  it('offers Quick Look for every external fallback state', () => {
    const onQuickLook = vi.fn();
    for (const content of [
      { kind: 'structuredFallback' as const, message: 'Open this workbook externally.' },
      { kind: 'videoExternal' as const },
    ]) {
      mount(
        baseAttrs(
          { status: 'ready', entry: entry(), content },
          { quickLookAvailable: true, onQuickLook },
        ),
      );
      root.querySelector<HTMLButtonElement>('.fm-file-viewer-quick-look')?.click();
    }

    expect(onQuickLook).toHaveBeenCalledTimes(2);
  });

  it('shows the exact error and external-open action for any preview failure', () => {
    const onOpenExternally = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'error',
          entry: entry(),
          message: 'File exceeds the preview size limit of 64 MiB.',
        },
        { onOpenExternally },
      ),
    );

    expect(root.querySelector('.fm-file-viewer-body')?.textContent).toContain(
      'File exceeds the preview size limit of 64 MiB.',
    );
    root.querySelector<HTMLButtonElement>('.fm-file-viewer-open-externally')?.click();
    expect(onOpenExternally).toHaveBeenCalledOnce();
  });

  it('renders text content and a search bar', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry(),
        content: {
          kind: 'text',
          windowOffset: 0,
          windowEnd: 5,
          text: 'hello',
          atStart: true,
          atEnd: true,
          loadingMore: false,
        },
      }),
    );
    expect(root.querySelector('.cm-content')?.textContent).toBe('hello');
    expect(root.querySelector('.fm-file-viewer-search-input')).not.toBeNull();
  });

  it('renders Markdown for F3 instead of showing its source', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'README.md', extension: 'md' }),
        content: {
          kind: 'text',
          windowOffset: 0,
          windowEnd: 7,
          text: '# Title',
          atStart: true,
          atEnd: true,
          loadingMore: false,
        },
      }),
    );

    expect(root.querySelector('.fm-file-viewer-markdown h1')?.textContent).toBe('Title');
    expect(root.querySelector('.fm-file-viewer-markdown')?.classList).toContain('browser-default');
    expect(root.querySelector('.cm-editor')).toBeNull();
  });

  it('highlights the active search match within the loaded window', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry(),
        content: {
          kind: 'text',
          windowOffset: 0,
          windowEnd: 11,
          text: 'hello world',
          atStart: true,
          atEnd: true,
          loadingMore: false,
          highlightOffset: 6,
          highlightLength: 5,
        },
        search: {
          query: 'world',
          regex: false,
          caseSensitive: false,
          wholeWord: false,
          matches: [{ offset: 6, length: 5, lineNumber: 1 }],
          truncated: false,
          currentMatchIndex: 0,
          searching: false,
          error: undefined,
        },
      }),
    );
    expect(root.querySelector('.cm-content')?.textContent).toBe('hello world');
    expect(root.querySelector('.fm-file-viewer-search-count')?.textContent).toBe('1 of 1');
  });

  it('renders image content sized to fit by default', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'photo.png', extension: 'png' }),
        content: {
          kind: 'image',
          dataUri: 'data:image/png;base64,AA==',
          zoom: 1,
          fitToContainer: true,
        },
      }),
    );
    const img = root.querySelector<HTMLImageElement>('.fm-file-viewer-body-image img');
    expect(img?.className).toContain('fm-file-viewer-image-fit');
    expect(root.querySelector('.fm-file-viewer-zoom-level')?.textContent).toBe('Fit');
  });

  it('shows the zoom percentage once zoomed and forwards zoom callbacks', () => {
    const onZoomIn = vi.fn();
    const onZoomOut = vi.fn();
    const onResetZoom = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry({ name: 'photo.png', extension: 'png' }),
          content: {
            kind: 'image',
            dataUri: 'data:image/png;base64,AA==',
            zoom: 1.25,
            fitToContainer: false,
          },
        },
        { onZoomIn, onZoomOut, onResetZoom },
      ),
    );
    expect(root.querySelector('.fm-file-viewer-zoom-level')?.textContent).toBe('125%');
    root.querySelector<HTMLButtonElement>('[data-tooltip="Zoom in"] button')?.click();
    root.querySelector<HTMLButtonElement>('[data-tooltip="Zoom out"] button')?.click();
    root.querySelector<HTMLButtonElement>('[data-tooltip="Fit to window"] button')?.click();
    expect(onZoomIn).toHaveBeenCalledTimes(1);
    expect(onZoomOut).toHaveBeenCalledTimes(1);
    expect(onResetZoom).toHaveBeenCalledTimes(1);
  });

  it('calls onLoadMore from bounded next-window navigation', () => {
    const onLoadMore = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry(),
          content: {
            kind: 'text',
            windowOffset: 0,
            windowEnd: 5,
            text: 'hello',
            atStart: true,
            atEnd: false,
            loadingMore: false,
          },
        },
        { onLoadMore },
      ),
    );
    const next = [
      ...root.querySelectorAll<HTMLButtonElement>('.fm-file-viewer-window-controls button'),
    ].find((button) => button.textContent === 'Next window');
    next?.click();
    expect(onLoadMore).toHaveBeenCalledTimes(1);
  });

  it('forwards search input, option toggles, and match navigation', () => {
    const onSearchQueryChange = vi.fn();
    const onSearchOptionChange = vi.fn();
    const onRunSearch = vi.fn();
    const onNextMatch = vi.fn();
    const onPreviousMatch = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry(),
          content: {
            kind: 'text',
            windowOffset: 0,
            windowEnd: 5,
            text: 'hello',
            atStart: true,
            atEnd: true,
            loadingMore: false,
          },
          search: {
            query: '',
            regex: false,
            caseSensitive: false,
            wholeWord: false,
            matches: [{ offset: 0, length: 1, lineNumber: 1 }],
            truncated: false,
            currentMatchIndex: 0,
            searching: false,
            error: undefined,
          },
        },
        { onSearchQueryChange, onSearchOptionChange, onRunSearch, onNextMatch, onPreviousMatch },
      ),
    );

    const input = root.querySelector<HTMLInputElement>('.fm-file-viewer-search-input');
    if (input === null) throw new Error('search input missing');
    input.value = 'cat';
    input.dispatchEvent(new InputEvent('input', { bubbles: true }));
    input.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onSearchQueryChange).toHaveBeenCalledWith('cat');
    expect(onRunSearch).toHaveBeenCalledTimes(1);

    root.querySelector<HTMLButtonElement>('button[title="Match case"]')?.click();
    expect(onSearchOptionChange).toHaveBeenCalledWith({ caseSensitive: true });

    root.querySelector<HTMLButtonElement>('button[title="Next match"]')?.click();
    root.querySelector<HTMLButtonElement>('button[title="Previous match"]')?.click();
    expect(onNextMatch).toHaveBeenCalledTimes(1);
    expect(onPreviousMatch).toHaveBeenCalledTimes(1);
  });

  it('calls onClose when the close button is clicked', () => {
    const onClose = vi.fn();
    mount(baseAttrs({ status: 'unsupported', entry: entry() }, { onClose }));
    root.querySelector<HTMLButtonElement>('.fm-file-viewer-close')?.click();
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('shows a copy button for text content and forwards onCopy', async () => {
    const onCopy = vi.fn().mockResolvedValue(undefined);
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry(),
          content: {
            kind: 'text',
            windowOffset: 0,
            windowEnd: 5,
            text: 'hello',
            atStart: true,
            atEnd: true,
            loadingMore: false,
          },
        },
        { onCopy },
      ),
    );
    const button = root.querySelector<HTMLButtonElement>('.fm-file-viewer-copy');
    expect(button?.getAttribute('aria-label')).toBe('Copy text');
    expect(root.querySelector('[data-tooltip="Copy text"]')).not.toBeNull();
    button?.click();
    await vi.waitFor(() => expect(onCopy).toHaveBeenCalledTimes(1));
  });

  it('shows a copy button for image content, labelled for images', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'photo.png', extension: 'png' }),
        content: {
          kind: 'image',
          dataUri: 'data:image/png;base64,AA==',
          zoom: 1,
          fitToContainer: true,
        },
      }),
    );
    expect(
      root.querySelector<HTMLButtonElement>('.fm-file-viewer-copy')?.getAttribute('aria-label'),
    ).toBe('Copy image');
  });

  it('renders PDF page navigation and forwards next/previousPage', () => {
    const onNextPage = vi.fn();
    const onPreviousPage = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry({ name: 'report.pdf', extension: 'pdf' }),
          content: {
            kind: 'pdf',
            document: {} as never,
            pageCount: 3,
            currentPage: 2,
          },
        },
        { onNextPage, onPreviousPage },
      ),
    );
    expect(root.querySelector('.fm-file-viewer-page-count')?.textContent).toBe('2 / 3');
    expect(root.querySelector('.fm-file-viewer-pdf-canvas')).not.toBeNull();
    root.querySelector<HTMLButtonElement>('[data-tooltip="Next page"] button')?.click();
    root.querySelector<HTMLButtonElement>('[data-tooltip="Previous page"] button')?.click();
    expect(onNextPage).toHaveBeenCalledTimes(1);
    expect(onPreviousPage).toHaveBeenCalledTimes(1);
  });

  it('disables PDF page navigation at the first/last page', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'report.pdf', extension: 'pdf' }),
        content: { kind: 'pdf', document: {} as never, pageCount: 1, currentPage: 1 },
      }),
    );
    expect(
      root.querySelector<HTMLButtonElement>('[data-tooltip="Previous page"] button')?.disabled,
    ).toBe(true);
    expect(
      root.querySelector<HTMLButtonElement>('[data-tooltip="Next page"] button')?.disabled,
    ).toBe(true);
  });

  it('re-renders the PDF canvas when the current page changes (regression: navigation used to only move the counter)', async () => {
    renderPdfPageToCanvas.mockClear();
    let currentAttrs = baseAttrs({
      status: 'ready',
      entry: entry({ name: 'report.pdf', extension: 'pdf' }),
      content: { kind: 'pdf', document: {} as never, pageCount: 3, currentPage: 1 },
    });
    m.mount(root, { view: () => m(FileViewer, currentAttrs) });
    await vi.waitFor(() =>
      expect(renderPdfPageToCanvas).toHaveBeenCalledWith(
        expect.anything(),
        1,
        expect.anything(),
        expect.anything(),
        expect.anything(),
      ),
    );

    currentAttrs = baseAttrs({
      status: 'ready',
      entry: entry({ name: 'report.pdf', extension: 'pdf' }),
      content: { kind: 'pdf', document: {} as never, pageCount: 3, currentPage: 2 },
    });
    m.redraw.sync();
    await vi.waitFor(() =>
      expect(renderPdfPageToCanvas).toHaveBeenCalledWith(
        expect.anything(),
        2,
        expect.anything(),
        expect.anything(),
        expect.anything(),
      ),
    );
  });

  it('shows an error instead of a silently blank canvas when a PDF page fails to render', async () => {
    renderPdfPageToCanvas.mockRejectedValueOnce(new Error('bad xref'));
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'report.pdf', extension: 'pdf' }),
        content: { kind: 'pdf', document: {} as never, pageCount: 1, currentPage: 1 },
      }),
    );
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-file-viewer-pdf-page-error')?.textContent).toContain(
        'bad xref',
      ),
    );
    renderPdfPageToCanvas.mockResolvedValue(undefined);
  });

  it('shows the PDF search bar and forwards query/navigation', () => {
    const onPdfSearchQueryChange = vi.fn();
    const onNextPdfMatch = vi.fn();
    const onPreviousPdfMatch = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry({ name: 'report.pdf', extension: 'pdf' }),
          content: { kind: 'pdf', document: {} as never, pageCount: 3, currentPage: 1 },
          pdfSearch: { query: 'foo', matches: [2, 3], currentMatchIndex: 0, searching: false },
        },
        { onPdfSearchQueryChange, onNextPdfMatch, onPreviousPdfMatch },
      ),
    );
    const input = root.querySelector<HTMLInputElement>('input[placeholder="Search this PDF…"]');
    expect(input?.value).toBe('foo');
    expect(root.querySelector('.fm-file-viewer-search-count')?.textContent).toBe('Page 2 · 1 of 2');
    input?.dispatchEvent(new InputEvent('input', { bubbles: true }));
    root.querySelector<HTMLButtonElement>('button[title="Next match"]')?.click();
    root.querySelector<HTMLButtonElement>('button[title="Previous match"]')?.click();
    expect(onPdfSearchQueryChange).toHaveBeenCalled();
    expect(onNextPdfMatch).toHaveBeenCalledTimes(1);
    expect(onPreviousPdfMatch).toHaveBeenCalledTimes(1);
  });

  it('renders a comic page image with page navigation', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'book.cbz', extension: 'cbz' }),
        content: {
          kind: 'comic',
          pageCount: 5,
          currentPage: 1,
          currentPageDataUri: 'data:image/jpeg;base64,AA==',
          loadingPage: false,
        },
      }),
    );
    expect(root.querySelector('.fm-file-viewer-page-count')?.textContent).toBe('2 / 5');
    const img = root.querySelector<HTMLImageElement>('.fm-file-viewer-body-image img');
    expect(img?.src).toContain('data:image/jpeg;base64,AA==');
  });

  it('shows a loading state for a comic page still being fetched', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'book.cbz', extension: 'cbz' }),
        content: {
          kind: 'comic',
          pageCount: 5,
          currentPage: 0,
          currentPageDataUri: undefined,
          loadingPage: true,
        },
      }),
    );
    expect(root.querySelector('.fm-file-viewer-body')?.textContent).toContain('Loading page');
  });

  it('renders an EPUB chapter with page navigation', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'book.epub', extension: 'epub' }),
        content: {
          kind: 'epub',
          title: 'My Book',
          chapterCount: 3,
          currentChapter: 1,
          currentChapterHtml: '<p>Chapter two content</p>',
          loadingChapter: false,
        },
      }),
    );
    expect(root.querySelector('.fm-file-viewer-page-count')?.textContent).toBe('2 / 3');
    expect(root.querySelector('.fm-file-viewer-epub-chapter')?.innerHTML).toContain(
      'Chapter two content',
    );
  });

  it('shows a loading state for an EPUB chapter still being fetched', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'book.epub', extension: 'epub' }),
        content: {
          kind: 'epub',
          title: undefined,
          chapterCount: 3,
          currentChapter: 0,
          currentChapterHtml: undefined,
          loadingChapter: true,
        },
      }),
    );
    expect(root.querySelector('.fm-file-viewer-body')?.textContent).toContain('Loading chapter');
  });

  it('does not show a copy button for audio content', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'song.mp3', extension: 'mp3' }),
        content: { kind: 'audio', dataUri: 'data:audio/mpeg;base64,AA==' },
      }),
    );
    expect(root.querySelector('.fm-file-viewer-copy')).toBeNull();
  });

  it('renders a small video with native controls, MIME-specific source, and thumbnail poster', () => {
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry({ name: 'clip.mp4', extension: 'mp4', size: 3 }),
          content: { kind: 'video', dataUri: 'data:video/mp4;base64,AQID' },
        },
        { videoPosterDataUri: 'data:image/jpeg;base64,BAUG' },
      ),
    );

    const video = root.querySelector<HTMLVideoElement>('.fm-file-viewer-body-video video');
    expect(video?.controls).toBe(true);
    expect(video?.getAttribute('src')).toBe('data:video/mp4;base64,AQID');
    expect(video?.getAttribute('poster')).toBe('data:image/jpeg;base64,BAUG');
  });

  it('opens an external-only video through the provided action', () => {
    const onOpenExternally = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry({ name: 'movie.mkv', extension: 'mkv', size: 3 }),
          content: { kind: 'videoExternal' },
        },
        { onOpenExternally },
      ),
    );

    expect(root.querySelector('.fm-file-viewer-body')?.textContent).toContain(
      'Large or unsupported video',
    );
    root.querySelector<HTMLButtonElement>('.fm-file-viewer-open-externally')?.click();
    expect(onOpenExternally).toHaveBeenCalledTimes(1);
  });

  it('virtualizes a million-row wide table while keeping a separate sticky header', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'large.csv', extension: 'csv' }),
        content: {
          kind: 'structuredTable',
          sessionId: 'structured-1',
          sourceBytes: 4_000_000_000,
          delimiter: ',',
          headerMode: 'firstRow',
          headers: Array.from({ length: 100 }, (_, index) => `Column ${index + 1}`),
          rows: Array.from({ length: 200 }, (_, row) => ({
            index: row,
            cells: Array.from({ length: 100 }, (_, column) => `${row}:${column}`),
          })),
          rowStart: 0,
          indexedRows: 1_000_000,
          totalRows: undefined,
          sourceIndexedRows: 1_000_000,
          sourceTotalRows: undefined,
          indexingComplete: false,
          loadingRows: false,
          warning: undefined,
          searchQuery: '',
          searchMatches: [],
          searchNextCursor: undefined,
          searching: false,
        },
      }),
    );

    expect(root.querySelector('.fm-structured-header-row')).not.toBeNull();
    expect(root.querySelectorAll('.fm-structured-row').length).toBeLessThan(50);
    expect(root.querySelectorAll('.fm-structured-header-cell').length).toBeLessThan(20);
    expect(root.querySelector<HTMLButtonElement>('button[title*="sorting"]')?.disabled).toBe(true);
  });

  it('renders populated Materialized delimiter and header controls', () => {
    const onStructuredOptionsChange = vi.fn();
    const onSortStructuredRows = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry({ name: 'small.csv', extension: 'csv' }),
          content: {
            kind: 'structuredTable',
            sessionId: 'structured-1',
            sourceBytes: 100,
            delimiter: ';',
            headerMode: 'firstRow',
            headers: ['name'],
            rows: [{ index: 0, cells: ['Ada'] }],
            rowStart: 0,
            indexedRows: 1,
            totalRows: 1,
            sourceIndexedRows: 1,
            sourceTotalRows: 1,
            indexingComplete: true,
            loadingRows: false,
            warning: undefined,
            searchQuery: '',
            searchMatches: [],
            searchNextCursor: undefined,
            searching: false,
          },
        },
        { onStructuredOptionsChange, onSortStructuredRows },
      ),
    );

    const values = Array.from(root.querySelectorAll<HTMLInputElement>('input.select-dropdown')).map(
      (input) => input.value,
    );
    expect(values).toEqual(['Semicolon', 'First row']);
    expect(root.querySelectorAll('select')).toHaveLength(0);

    root.querySelectorAll<HTMLInputElement>('input.select-dropdown')[0]?.click();
    m.redraw.sync();
    Array.from(root.querySelectorAll('li'))
      .find((item) => item.textContent?.includes('Pipe'))
      ?.click();
    expect(onStructuredOptionsChange).toHaveBeenCalledWith('|', 'firstRow');

    root.querySelector<HTMLButtonElement>('.fm-structured-sort')?.click();
    expect(onSortStructuredRows).toHaveBeenCalledWith(0);
  });

  it('renders workbook sheet tabs and formula source beside its cached value', () => {
    const onSelectStructuredSheet = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry({ name: 'budget.xlsx', extension: 'xlsx' }),
          content: {
            kind: 'structuredTable',
            sessionId: 'excel-1',
            sourceBytes: 10_000,
            delimiter: ',',
            headerMode: 'none',
            headers: ['A'],
            rows: [
              {
                index: 0,
                cells: ['84'],
                cellDetails: [{ column: 0, display: '84', valueType: 'number', formula: 'B1*2' }],
              },
            ],
            sheets: [
              { name: 'Summary', rowCount: 1, columnCount: 1 },
              { name: 'Details', rowCount: 3, columnCount: 1 },
            ],
            selectedSheet: 'Summary',
            rowStart: 0,
            indexedRows: 1,
            totalRows: 1,
            sourceIndexedRows: 1,
            sourceTotalRows: 1,
            indexingComplete: true,
            loadingRows: false,
            warning: undefined,
            searchQuery: '',
            searchMatches: [],
            searchNextCursor: undefined,
            searching: false,
          },
        },
        { onSelectStructuredSheet },
      ),
    );

    expect(root.querySelectorAll('.fm-structured-sheet-tab')).toHaveLength(2);
    expect(root.querySelector('.fm-structured-toolbar .fm-structured-option')).toBeNull();
    expect(
      root.querySelector('.fm-structured-row .fm-structured-cell')?.getAttribute('title'),
    ).toContain('B1*2');
    Array.from(root.querySelectorAll<HTMLButtonElement>('.fm-structured-sheet-tab'))
      .find((button) => button.textContent === 'Details')
      ?.click();
    expect(onSelectStructuredSheet).toHaveBeenCalledWith('Details');
  });

  it('renders wrapped highlighted JSON with Materialized previous and next controls', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'events.jsonl', extension: 'jsonl' }),
        content: {
          kind: 'structuredJson',
          sessionId: 'json-session',
          data: Array.from(new TextEncoder().encode('{"city":"Zürich"}')),
          tokens: [
            { kind: 'property', start: 1, length: 6 },
            { kind: 'string', start: 8, length: 9 },
          ],
          windowOffset: 0,
          sourceBytes: 100_000,
          atStart: true,
          atEnd: false,
          loadingWindow: false,
          warning: undefined,
        },
      }),
    );

    expect(root.querySelector('.fm-json-token-property')?.textContent).toBe('"city"');
    expect(root.querySelector('.fm-structured-json-source-wrap')).not.toBeNull();
    expect(root.querySelector('.fm-structured-window-previous')).not.toBeNull();
    expect(root.querySelector('.fm-structured-window-next')).not.toBeNull();
  });

  describe('markdown search-match highlighting', () => {
    // Rendered markdown stays rendered even with a match highlighted (unlike an earlier approach
    // that fell back to CodeMirror source, which was less intuitive - you'd see rendered HTML,
    // search, and suddenly see raw markdown). The highlight itself is applied via the CSS Custom
    // Highlight API in `markdown-search-highlight.ts`, which has its own focused unit tests for
    // the actual match-locating logic; here we only check the viewer keeps rendering HTML.
    function markdownState(overrides: Partial<FileViewerState> = {}): FileViewerState {
      return {
        status: 'ready',
        entry: entry({ name: 'README.md', extension: 'md' }),
        content: {
          kind: 'text',
          windowOffset: 0,
          windowEnd: 20,
          text: '# Title\n\nhello world',
          atStart: true,
          atEnd: true,
          loadingMore: false,
        },
        ...overrides,
      } as FileViewerState;
    }

    it('stays rendered as HTML (not CodeMirror source) once a search match is highlighted', () => {
      mount(
        baseAttrs(
          markdownState({
            content: {
              kind: 'text',
              windowOffset: 0,
              windowEnd: 20,
              text: '# Title\n\nhello world',
              atStart: true,
              atEnd: true,
              loadingMore: false,
              highlightOffset: 15,
              highlightLength: 5,
            },
          } as Partial<FileViewerState>),
        ),
      );

      expect(root.querySelector('.fm-file-viewer-markdown h1')?.textContent).toBe('Title');
      expect(root.querySelector('.cm-editor')).toBeNull();
    });

    it('renders normally with no active search', () => {
      mount(baseAttrs(markdownState()));

      expect(root.querySelector('.fm-file-viewer-markdown h1')?.textContent).toBe('Title');
      expect(root.querySelector('.cm-editor')).toBeNull();
    });
  });
});
