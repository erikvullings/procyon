import m from 'mithril';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import type { EntrySummary } from '../../models';
import { FileViewer, type FileViewerAttrs } from './file-viewer';
import type { FileViewerState } from './file-viewer-controller';

const renderPdfPageToCanvas = vi.fn().mockResolvedValue(undefined);
const renderPdfSearchHighlights = vi.fn().mockResolvedValue(undefined);
vi.mock('./pdf-preview', () => ({
  renderPdfPageToCanvas: (...args: unknown[]) => renderPdfPageToCanvas(...args),
  renderPdfSearchHighlights: (...args: unknown[]) => renderPdfSearchHighlights(...args),
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
    onLoadTextPage: vi.fn(),
    onLoadStructuredRows: vi.fn(),
    onStructuredOptionsChange: vi.fn(),
    onSelectStructuredSheet: vi.fn(),
    onToggleStructuredRowNumbers: vi.fn(),
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
    onZoomChange: vi.fn(),
    onResetZoom: vi.fn(),
    onCopy: vi.fn().mockResolvedValue(undefined),
    onToggleMetadata: vi.fn(),
    onNextPage: vi.fn(),
    onPreviousPage: vi.fn(),
    onPdfSearchQueryChange: vi.fn(),
    onNextPdfMatch: vi.fn(),
    onPreviousPdfMatch: vi.fn(),
    onEpubSearchQueryChange: vi.fn(),
    onNextEpubMatch: vi.fn(),
    onPreviousEpubMatch: vi.fn(),
    onSelectEpubSection: vi.fn(),
    onFollowEpubLink: vi.fn(),
    onOpenExternalLink: vi.fn(),
    onSelectPdfPage: vi.fn(),
    onNavigateTextOffset: vi.fn(),
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

function toggleSearch(): void {
  root
    .querySelector('.fm-file-viewer')
    ?.dispatchEvent(new CustomEvent('fm-viewer-toggle-search', { bubbles: true }));
  m.redraw.sync();
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

  it('renders text content with search closed by default', () => {
    const onSearchQueryChange = vi.fn();
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
        { onSearchQueryChange },
      ),
    );
    expect(root.querySelector('.cm-content')?.textContent).toBe('hello');
    expect(root.querySelector('.fm-file-viewer-search-input')).toBeNull();
    toggleSearch();
    expect(root.querySelector('.fm-file-viewer-search-input')).not.toBeNull();
    toggleSearch();
    expect(root.querySelector('.fm-file-viewer-search-input')).toBeNull();
    expect(onSearchQueryChange).toHaveBeenCalledWith('');
    expect(root.querySelector('.fm-file-viewer-text-pagination')).toBeNull();
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
    root.querySelector<HTMLButtonElement>('.fm-file-viewer-outline-toggle')?.click();
    m.redraw.sync();
    expect(root.querySelector('.fm-file-viewer-outline-item')?.textContent).toContain('Title');
  });

  it('builds a navigable outline for HTML source headings', () => {
    const onNavigateTextOffset = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry({ name: 'guide.html', extension: 'html' }),
          content: {
            kind: 'text',
            windowOffset: 0,
            windowEnd: 34,
            text: '<h1>Start</h1><p>Text</p><h2>Next</h2>',
            atStart: true,
            atEnd: true,
            loadingMore: false,
          },
        },
        { onNavigateTextOffset },
      ),
    );

    root.querySelector<HTMLButtonElement>('.fm-file-viewer-outline-toggle')?.click();
    m.redraw.sync();
    const items = root.querySelectorAll<HTMLButtonElement>('.fm-file-viewer-outline-item');
    expect([...items].map((item) => item.textContent)).toEqual(['1Start', '2Next']);
    items[1]?.click();
    expect(onNavigateTextOffset).toHaveBeenCalledWith(25, 13);
  });

  it('builds a navigable outline for DOCX headings', () => {
    const scrollIntoView = vi.fn();
    HTMLElement.prototype.scrollIntoView = scrollIntoView;
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'guide.docx', extension: 'docx' }),
        content: {
          kind: 'docx',
          sessionId: 'docx-1',
          sourceHtml: '<h1>Start</h1><p>Text</p><h2>Next</h2>',
          html: '<h1>Start</h1><p>Text</p><h2>Next</h2>',
          plainText: 'Start\nText\nNext',
          omittedFeatures: [],
        },
      }),
    );

    root.querySelector<HTMLButtonElement>('.fm-file-viewer-outline-toggle')?.click();
    m.redraw.sync();
    const items = root.querySelectorAll<HTMLButtonElement>('.fm-file-viewer-outline-item');
    expect([...items].map((item) => item.textContent)).toEqual(['1Start', '2Next']);
    items[1]?.click();
    expect(scrollIntoView).toHaveBeenCalledWith({ block: 'start', inline: 'nearest' });
  });

  it('selects and copies rendered Markdown without bubbling into file shortcuts', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'README.md', extension: 'md' }),
        content: {
          kind: 'text',
          windowOffset: 0,
          windowEnd: 18,
          text: '# Selectable text',
          atStart: true,
          atEnd: true,
          loadingMore: false,
        },
      }),
    );
    const markdown = root.querySelector<HTMLElement>('.fm-file-viewer-markdown');
    if (markdown === null) throw new Error('Markdown preview missing');
    expect(markdown.tabIndex).toBe(0);
    const selectAll = new KeyboardEvent('keydown', {
      key: 'a',
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    markdown.dispatchEvent(selectAll);
    expect(selectAll.defaultPrevented).toBe(true);
    expect(document.getSelection()?.toString()).toContain('Selectable text');

    const bubbledCopy = vi.fn();
    root.addEventListener('keydown', bubbledCopy);
    const copy = new KeyboardEvent('keydown', {
      key: 'c',
      metaKey: true,
      bubbles: true,
      cancelable: true,
    });
    markdown.dispatchEvent(copy);
    expect(copy.defaultPrevented).toBe(false);
    expect(bubbledCopy).not.toHaveBeenCalled();
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

  it('shows page-based text pagination only when the file has multiple windows', () => {
    const onLoadTextPage = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry({ size: 3 * 64 * 1024 }),
          content: {
            kind: 'text',
            windowOffset: 64 * 1024,
            windowEnd: 128 * 1024,
            text: 'hello',
            atStart: false,
            atEnd: false,
            loadingMore: false,
          },
        },
        { onLoadTextPage },
      ),
    );
    const pagination = root.querySelector('.fm-file-viewer-text-pagination');
    expect(pagination?.textContent).toContain('Page 2 of 3');
    expect(pagination?.textContent).not.toContain('bytes');
    pagination?.querySelectorAll<HTMLButtonElement>('.pagination-controls button')[2]?.click();
    expect(onLoadTextPage).toHaveBeenCalledWith(2);
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

    toggleSearch();

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

  it('renders PDF bookmarks and supports direct page entry', () => {
    const onNextPage = vi.fn();
    const onPreviousPage = vi.fn();
    const onSelectPdfPage = vi.fn();
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
            zoom: 1,
            outline: [
              { label: 'Introduction', level: 1, page: 1 },
              { label: 'Details', level: 2, page: 3 },
            ],
          },
        },
        { onNextPage, onPreviousPage, onSelectPdfPage },
      ),
    );
    expect(root.querySelector('.fm-file-viewer-page-count')?.textContent).toBe('2 / 3');
    expect(root.querySelector('.fm-file-viewer-pdf-canvas')).not.toBeNull();
    root.querySelector<HTMLButtonElement>('[data-tooltip="Next page"] button')?.click();
    root.querySelector<HTMLButtonElement>('[data-tooltip="Previous page"] button')?.click();
    expect(onNextPage).toHaveBeenCalledTimes(1);
    expect(onPreviousPage).toHaveBeenCalledTimes(1);
    root.querySelector<HTMLButtonElement>('.fm-file-viewer-outline-toggle')?.click();
    m.redraw.sync();
    const outlineItems = root.querySelectorAll<HTMLButtonElement>('.fm-file-viewer-outline-item');
    expect(outlineItems).toHaveLength(2);
    expect(outlineItems[0]?.textContent).toContain('Introduction');
    outlineItems[1]?.click();
    expect(onSelectPdfPage).toHaveBeenCalledWith(3);

    root.querySelector<HTMLButtonElement>('.fm-file-viewer-page-count')?.click();
    m.redraw.sync();
    const pageInput = root.querySelector<HTMLInputElement>('.fm-file-viewer-page-input');
    if (pageInput === null) throw new Error('page input missing');
    expect(pageInput.type).toBe('text');
    expect(pageInput.max).toBe('3');
    pageInput.value = '3';
    pageInput.dispatchEvent(new InputEvent('input', { bubbles: true }));
    m.redraw.sync();
    expect(root.querySelector<HTMLInputElement>('.fm-file-viewer-page-input')?.value).toBe('3');
    pageInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onSelectPdfPage).toHaveBeenCalledWith(3);
  });

  it('disables PDF page navigation at the first/last page', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'report.pdf', extension: 'pdf' }),
        content: { kind: 'pdf', document: {} as never, pageCount: 1, currentPage: 1, zoom: 1 },
      }),
    );
    expect(
      root.querySelector<HTMLButtonElement>('[data-tooltip="Previous page"] button')?.disabled,
    ).toBe(true);
    expect(
      root.querySelector<HTMLButtonElement>('[data-tooltip="Next page"] button')?.disabled,
    ).toBe(true);
  });

  it('keeps the rendered PDF canvas mounted when search opens and disables suggestions', () => {
    const displayDocument = {} as never;
    let currentAttrs = baseAttrs({
      status: 'ready',
      entry: entry({ name: 'report.pdf', extension: 'pdf' }),
      content: { kind: 'pdf', document: displayDocument, pageCount: 1, currentPage: 1, zoom: 1 },
    });
    m.mount(root, { view: () => m(FileViewer, currentAttrs) });
    const canvas = root.querySelector('.fm-file-viewer-pdf-canvas');

    toggleSearch();

    expect(root.querySelector('.fm-file-viewer-pdf-canvas')).toBe(canvas);
    const input = root.querySelector<HTMLInputElement>('.fm-file-viewer-search-input');
    expect(input?.autocomplete).toBe('new-password');
    expect(input?.getAttribute('autocorrect')).toBe('off');
    expect(input?.getAttribute('spellcheck')).toBe('false');
    expect((input as HTMLInputElement & { autocorrect: boolean }).autocorrect).toBe(false);
    expect(input?.spellcheck).toBe(false);

    currentAttrs = baseAttrs({
      status: 'ready',
      entry: entry({ name: 'report.pdf', extension: 'pdf' }),
      content: {
        kind: 'pdf',
        document: displayDocument,
        searchDocument: {} as never,
        pageCount: 1,
        currentPage: 1,
        zoom: 1,
      },
      pdfSearch: {
        query: 'process',
        regex: false,
        caseSensitive: false,
        wholeWord: false,
        matches: [],
        currentMatchIndex: undefined,
        searching: true,
        error: undefined,
      },
    });
    m.redraw.sync();

    expect(root.querySelector('.fm-file-viewer-pdf-canvas')).toBe(canvas);
  });

  it('re-renders the PDF canvas when the current page changes (regression: navigation used to only move the counter)', async () => {
    renderPdfPageToCanvas.mockClear();
    let currentAttrs = baseAttrs({
      status: 'ready',
      entry: entry({ name: 'report.pdf', extension: 'pdf' }),
      content: { kind: 'pdf', document: {} as never, pageCount: 3, currentPage: 1, zoom: 1 },
    });
    m.mount(root, { view: () => m(FileViewer, currentAttrs) });
    await vi.waitFor(() =>
      expect(renderPdfPageToCanvas).toHaveBeenCalledWith(
        expect.anything(),
        1,
        expect.anything(),
        expect.anything(),
        expect.anything(),
        1,
      ),
    );

    currentAttrs = baseAttrs({
      status: 'ready',
      entry: entry({ name: 'report.pdf', extension: 'pdf' }),
      content: { kind: 'pdf', document: {} as never, pageCount: 3, currentPage: 2, zoom: 1 },
    });
    m.redraw.sync();
    await vi.waitFor(() =>
      expect(renderPdfPageToCanvas).toHaveBeenCalledWith(
        expect.anything(),
        2,
        expect.anything(),
        expect.anything(),
        expect.anything(),
        1,
      ),
    );

    currentAttrs = baseAttrs({
      status: 'ready',
      entry: entry({ name: 'report.pdf', extension: 'pdf' }),
      content: { kind: 'pdf', document: {} as never, pageCount: 3, currentPage: 2, zoom: 1.25 },
    });
    m.redraw.sync();
    await vi.waitFor(() =>
      expect(renderPdfPageToCanvas).toHaveBeenCalledWith(
        expect.anything(),
        2,
        expect.anything(),
        expect.anything(),
        expect.anything(),
        1.25,
      ),
    );
    expect(root.querySelector('.fm-file-viewer-zoom-level')?.textContent).toBe('125%');
  });

  it('renders search highlights only after a search-driven page change finishes rendering', async () => {
    const displayDocument = {} as never;
    const searchDocument = {} as never;
    let currentAttrs = baseAttrs({
      status: 'ready',
      entry: entry({ name: 'report.pdf', extension: 'pdf' }),
      content: { kind: 'pdf', document: displayDocument, pageCount: 2, currentPage: 1, zoom: 1 },
    });
    m.mount(root, { view: () => m(FileViewer, currentAttrs) });
    await vi.waitFor(() => expect(renderPdfPageToCanvas).toHaveBeenCalled());
    await vi.waitFor(() => expect(renderPdfSearchHighlights).toHaveBeenCalled());
    renderPdfSearchHighlights.mockClear();
    let finishPageRender: (() => void) | undefined;
    renderPdfPageToCanvas.mockImplementationOnce(
      () =>
        new Promise<void>((resolve) => {
          finishPageRender = resolve;
        }),
    );

    currentAttrs = baseAttrs({
      status: 'ready',
      entry: entry({ name: 'report.pdf', extension: 'pdf' }),
      content: {
        kind: 'pdf',
        document: displayDocument,
        searchDocument,
        pageCount: 2,
        currentPage: 2,
        zoom: 1,
      },
      pdfSearch: {
        query: 'process',
        regex: false,
        caseSensitive: false,
        wholeWord: false,
        matches: [{ pageNumber: 2, occurrenceIndex: 0 }],
        currentMatchIndex: 0,
        searching: false,
        error: undefined,
      },
    });
    m.redraw.sync();

    expect(renderPdfSearchHighlights).not.toHaveBeenCalled();
    finishPageRender?.();
    await vi.waitFor(() => expect(renderPdfSearchHighlights).toHaveBeenCalled());
  });

  it('shows an error instead of a silently blank canvas when a PDF page fails to render', async () => {
    renderPdfPageToCanvas.mockRejectedValueOnce(new Error('bad xref'));
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'report.pdf', extension: 'pdf' }),
        content: { kind: 'pdf', document: {} as never, pageCount: 1, currentPage: 1, zoom: 1 },
      }),
    );
    await vi.waitFor(() =>
      expect(root.querySelector('.fm-file-viewer-pdf-page-error')?.textContent).toContain(
        'bad xref',
      ),
    );
    renderPdfPageToCanvas.mockResolvedValue(undefined);
  });

  it('renders PDF search highlights from the isolated search document', async () => {
    renderPdfSearchHighlights.mockClear();
    const displayDocument = {} as never;
    const searchDocument = {} as never;
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'report.pdf', extension: 'pdf' }),
        content: {
          kind: 'pdf',
          document: displayDocument,
          searchDocument,
          pageCount: 1,
          currentPage: 1,
          zoom: 1,
        },
        pdfSearch: {
          query: 'triz',
          regex: false,
          caseSensitive: false,
          wholeWord: false,
          matches: [
            { pageNumber: 1, occurrenceIndex: 0 },
            { pageNumber: 1, occurrenceIndex: 1 },
          ],
          currentMatchIndex: 0,
          searching: false,
          error: undefined,
        },
      }),
    );

    await vi.waitFor(() => expect(renderPdfSearchHighlights).toHaveBeenCalled());
    const [document, pageNumber, container, , , expression, activeOccurrenceIndex, zoom] =
      renderPdfSearchHighlights.mock.calls.at(-1) ?? [];
    expect(document).toBe(searchDocument);
    expect(pageNumber).toBe(1);
    expect(container).toBeInstanceOf(HTMLElement);
    expect(expression).toBeInstanceOf(RegExp);
    expect((expression as RegExp).test('TRIZ')).toBe(true);
    expect(activeOccurrenceIndex).toBe(0);
    expect(zoom).toBe(1);
  });

  it('opens the generic PDF search bar on request and forwards options, query, and navigation', () => {
    const onPdfSearchQueryChange = vi.fn();
    const onNextPdfMatch = vi.fn();
    const onPreviousPdfMatch = vi.fn();
    const onSearchOptionChange = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry({ name: 'report.pdf', extension: 'pdf' }),
          content: { kind: 'pdf', document: {} as never, pageCount: 3, currentPage: 1, zoom: 1 },
          pdfSearch: {
            query: 'foo',
            regex: false,
            caseSensitive: false,
            wholeWord: false,
            matches: [
              { pageNumber: 2, occurrenceIndex: 0 },
              { pageNumber: 3, occurrenceIndex: 0 },
            ],
            currentMatchIndex: 0,
            searching: false,
            error: undefined,
          },
        },
        {
          onPdfSearchQueryChange,
          onNextPdfMatch,
          onPreviousPdfMatch,
          onSearchOptionChange,
        },
      ),
    );
    expect(root.querySelector('.fm-file-viewer-search')).not.toBeNull();
    const input = root.querySelector<HTMLInputElement>('.fm-file-viewer-search-input');
    expect(input?.value).toBe('foo');
    expect(root.querySelector('.fm-file-viewer-search-count')?.textContent).toBe('Page 2 · 1 of 2');
    input?.dispatchEvent(new InputEvent('input', { bubbles: true }));
    root.querySelector<HTMLButtonElement>('button[title="Next match"]')?.click();
    root.querySelector<HTMLButtonElement>('button[title="Previous match"]')?.click();
    expect(onPdfSearchQueryChange).toHaveBeenCalled();
    expect(onNextPdfMatch).toHaveBeenCalledTimes(1);
    expect(onPreviousPdfMatch).toHaveBeenCalledTimes(1);
    root.querySelector<HTMLButtonElement>('button[title="Match case"]')?.click();
    expect(onSearchOptionChange).toHaveBeenCalledWith({ caseSensitive: true });
    root.querySelector<HTMLButtonElement>('.fm-file-viewer-search-close')?.click();
    m.redraw.sync();
    expect(root.querySelector('.fm-file-viewer-search')).toBeNull();
    expect(onPdfSearchQueryChange).toHaveBeenLastCalledWith('');
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

  it('renders an EPUB chapter with navigation, zoom, focus, and inline search', () => {
    const onEpubSearchQueryChange = vi.fn();
    const onNextEpubMatch = vi.fn();
    const onSelectEpubSection = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry({ name: 'book.epub', extension: 'epub' }),
          content: {
            kind: 'epub',
            title: 'My Book',
            chapterCount: 3,
            currentChapter: 1,
            currentChapterHtml: '<p>Chapter two content</p>',
            loadingChapter: false,
            zoom: 1.25,
            sectionLabels: [undefined, 'Chapter Two', 'Final thoughts'],
          },
          epubSearch: {
            query: 'chapter',
            matches: [
              { chapterNumber: 2, occurrenceIndex: 0 },
              { chapterNumber: 3, occurrenceIndex: 0 },
            ],
            currentMatchIndex: 0,
            searching: false,
          },
        },
        { onEpubSearchQueryChange, onNextEpubMatch, onSelectEpubSection },
      ),
    );
    expect(root.querySelector('.fm-file-viewer-page-count')?.textContent).toBe('2 / 3');
    const chapter = root.querySelector<HTMLElement>('.fm-file-viewer-epub-chapter');
    expect(chapter?.textContent).toContain('Chapter two content');
    expect(chapter?.style.fontSize).toBe('1.25em');
    expect(root.querySelector('.fm-file-viewer-body-epub')?.getAttribute('tabindex')).toBe('0');
    expect(root.querySelector('.fm-file-viewer-search-count')?.textContent).toBe(
      'Chapter 2 · 1 of 2',
    );
    const input = root.querySelector<HTMLInputElement>('.fm-file-viewer-search-input');
    input?.dispatchEvent(new InputEvent('input', { bubbles: true }));
    root.querySelector<HTMLButtonElement>('button[title="Next match"]')?.click();
    expect(onEpubSearchQueryChange).toHaveBeenCalledOnce();
    expect(onNextEpubMatch).toHaveBeenCalledOnce();

    root.querySelector<HTMLButtonElement>('.fm-file-viewer-page-count')?.click();
    m.redraw.sync();
    const sectionInput = root.querySelector<HTMLInputElement>('.fm-file-viewer-page-input');
    if (sectionInput === null) throw new Error('section input missing');
    expect(sectionInput.min).toBe('1');
    expect(sectionInput.max).toBe('3');
    sectionInput.value = '3';
    sectionInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onSelectEpubSection).toHaveBeenCalledWith(2);
  });

  it('highlights EPUB matches in rendered markup and reveals the active occurrence', () => {
    const scrollIntoView = vi.fn();
    HTMLElement.prototype.scrollIntoView = scrollIntoView;
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'book.epub', extension: 'epub' }),
        content: {
          kind: 'epub',
          title: 'Book',
          chapterCount: 1,
          currentChapter: 0,
          currentChapterHtml: '<p>Chapter <strong>chapter</strong></p>',
          loadingChapter: false,
          zoom: 1,
          sectionLabels: ['Chapter'],
        },
        epubSearch: {
          query: 'chapter',
          regex: false,
          caseSensitive: false,
          wholeWord: true,
          matches: [
            { chapterNumber: 1, occurrenceIndex: 0 },
            { chapterNumber: 1, occurrenceIndex: 1 },
          ],
          currentMatchIndex: 1,
          searching: false,
        },
      }),
    );

    const chapter = root.querySelector('.fm-file-viewer-epub-chapter');
    expect(chapter?.querySelectorAll('.fm-document-search-match')).toHaveLength(1);
    expect(chapter?.querySelectorAll('.fm-document-search-match-active')).toHaveLength(1);
    expect(chapter?.querySelector('strong .fm-document-search-match-active')?.textContent).toBe(
      'chapter',
    );
    expect(scrollIntoView).toHaveBeenCalledWith({ block: 'center', inline: 'nearest' });
  });

  it('edits zoom inline without changing toolbar height', () => {
    const onZoomChange = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry({ name: 'book.epub', extension: 'epub' }),
          content: {
            kind: 'epub',
            title: 'Book',
            chapterCount: 1,
            currentChapter: 0,
            currentChapterHtml: '<p>Text</p>',
            loadingChapter: false,
            zoom: 1,
            sectionLabels: ['Text'],
          },
        },
        { onZoomChange },
      ),
    );
    root.querySelector<HTMLButtonElement>('.fm-file-viewer-zoom-level')?.click();
    m.redraw.sync();
    const zoomInput = root.querySelector<HTMLInputElement>('.fm-file-viewer-zoom-input');
    if (zoomInput === null) throw new Error('zoom input missing');
    zoomInput.value = '114';
    zoomInput.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    expect(onZoomChange).toHaveBeenCalledWith(1.14);
  });

  it('opens the EPUB ToC and routes internal and confirmed external links', () => {
    const onSelectEpubSection = vi.fn();
    const onFollowEpubLink = vi.fn();
    const onOpenExternalLink = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry({ name: 'book.epub', extension: 'epub' }),
          content: {
            kind: 'epub',
            title: 'My Book',
            chapterCount: 3,
            currentChapter: 0,
            currentChapterHtml:
              '<p><a href="chapter2.xhtml#note">Next chapter</a> <a href="https://example.com/read">Publisher</a></p>',
            loadingChapter: false,
            zoom: 1,
            sectionLabels: ['Introduction', 'Chapter Two', undefined],
          },
        },
        { onSelectEpubSection, onFollowEpubLink, onOpenExternalLink },
      ),
    );

    root.querySelector<HTMLButtonElement>('.fm-file-viewer-outline-toggle')?.click();
    m.redraw.sync();
    const tocItems = root.querySelectorAll<HTMLButtonElement>('.fm-file-viewer-outline-item');
    expect(tocItems).toHaveLength(3);
    expect(tocItems[1]?.textContent).toContain('Chapter Two');
    tocItems[1]?.click();
    expect(onSelectEpubSection).toHaveBeenCalledWith(1);

    root.querySelector<HTMLAnchorElement>('a[href^="chapter2"]')?.click();
    expect(onFollowEpubLink).toHaveBeenCalledWith('chapter2.xhtml#note');

    root.querySelector<HTMLAnchorElement>('a[href^="https"]')?.click();
    m.redraw.sync();
    expect(onOpenExternalLink).not.toHaveBeenCalled();
    const confirm = [...root.querySelectorAll<HTMLButtonElement>('button')].find(
      (button) => button.textContent === 'Open link',
    );
    confirm?.click();
    expect(onOpenExternalLink).toHaveBeenCalledWith('https://example.com/read');
  });

  it('routes fragment-level EPUB ToC entries to the correct section anchor', () => {
    const onSelectEpubSection = vi.fn();
    mount(
      baseAttrs(
        {
          status: 'ready',
          entry: entry({ name: 'book.epub', extension: 'epub' }),
          content: {
            kind: 'epub',
            title: 'My Book',
            chapterCount: 2,
            currentChapter: 0,
            currentChapterHtml: '<h1 id="intro">Introduction</h1>',
            loadingChapter: false,
            zoom: 1,
            sectionLabels: ['1 Introduction', '2 Next chapter'],
            outline: [
              { label: '1 Introduction', level: 1, chapterIndex: 0 },
              { label: '1.4 Summary', level: 2, chapterIndex: 0, fragment: 'ch1.4' },
            ],
          },
        },
        { onSelectEpubSection },
      ),
    );

    root.querySelector<HTMLButtonElement>('.fm-file-viewer-outline-toggle')?.click();
    m.redraw.sync();
    const items = root.querySelectorAll<HTMLButtonElement>('.fm-file-viewer-outline-item');
    expect(items).toHaveLength(2);
    items[1]?.click();
    expect(onSelectEpubSection).toHaveBeenCalledWith(0, 'ch1.4');
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
          zoom: 1,
          sectionLabels: [undefined, undefined, undefined],
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
    const onToggleStructuredRowNumbers = vi.fn();
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
        { onSelectStructuredSheet, onToggleStructuredRowNumbers },
      ),
    );

    expect(root.querySelectorAll('.fm-structured-sheet-tab')).toHaveLength(2);
    expect(
      root
        .querySelector('.fm-structured-view')
        ?.lastElementChild?.classList.contains('fm-structured-sheet-tabs'),
    ).toBe(true);
    expect(
      root
        .querySelector('.fm-structured-view')
        ?.firstElementChild?.classList.contains('fm-structured-toolbar'),
    ).toBe(true);
    expect(
      root
        .querySelector('.fm-structured-toolbar')
        ?.firstElementChild?.classList.contains('fm-structured-search'),
    ).toBe(true);
    expect(
      root.querySelector('.fm-structured-toolbar .fm-structured-row-number-toggle'),
    ).toBeNull();
    expect(
      root.querySelector('.fm-structured-sheet-tabs .fm-structured-row-number-toggle'),
    ).not.toBeNull();
    expect(root.querySelector('.fm-structured-row-number-toggle')?.textContent).toBe('Rows');
    expect(
      root.querySelector('.fm-structured-row-number-control')?.getAttribute('data-tooltip'),
    ).toBe('Show row numbers');
    expect(
      root
        .querySelector('.fm-structured-row-number-control')
        ?.getAttribute('data-tooltip-placement'),
    ).toBe('above');
    expect(root.querySelector('.fm-structured-row-number-toggle')?.lastElementChild?.tagName).toBe(
      'INPUT',
    );
    expect(root.querySelector('.fm-structured-toolbar .fm-structured-option')).toBeNull();
    expect(root.querySelector('.fm-structured-row-number-cell')).toBeNull();
    expect(
      root.querySelector('.fm-structured-row .fm-structured-cell')?.getAttribute('title'),
    ).toContain('B1*2');
    root.querySelector<HTMLInputElement>('.fm-structured-row-number-toggle input')?.click();
    expect(onToggleStructuredRowNumbers).toHaveBeenCalledTimes(1);
    Array.from(root.querySelectorAll<HTMLButtonElement>('.fm-structured-sheet-tab'))
      .find((button) => button.textContent === 'Details')
      ?.click();
    expect(onSelectStructuredSheet).toHaveBeenCalledWith('Details');
  });

  it('shows an optional row-number column and the shared sort caret', () => {
    mount(
      baseAttrs({
        status: 'ready',
        entry: entry({ name: 'budget.xlsx', extension: 'xlsx' }),
        content: {
          kind: 'structuredTable',
          sessionId: 'excel-1',
          sourceBytes: 10_000,
          delimiter: ',',
          headerMode: 'none',
          headers: ['A'],
          rows: [{ index: 0, cells: ['Value'] }],
          sheets: [{ name: 'Summary', rowCount: 6_717, columnCount: 1 }],
          selectedSheet: 'Summary',
          rowStart: 0,
          indexedRows: 1,
          totalRows: 6_717,
          sourceIndexedRows: 1,
          sourceTotalRows: 6_717,
          indexingComplete: true,
          loadingRows: false,
          warning: undefined,
          searchQuery: '',
          searchMatches: [],
          searchNextCursor: undefined,
          searching: false,
          showRowNumbers: true,
          sortColumn: 0,
          sortDirection: 'ascending',
        },
      }),
    );

    expect(root.querySelector('.fm-structured-header-row-number')?.textContent).toBe('#');
    expect(root.querySelector('.fm-structured-row-number-cell')?.textContent).toBe('1');
    expect(root.querySelector('.fm-structured-row-number-cell')?.getAttribute('style')).toContain(
      'width: 36px',
    );
    expect(root.querySelector('.fm-structured-row')?.getAttribute('style')).toContain(
      'height: 20px',
    );
    expect(root.querySelector('.fm-structured-sort .fm-sort-indicator')).not.toBeNull();
  });

  it('uses the directory table zebra stripe pattern for spreadsheet rows', () => {
    mount(
      baseAttrs({
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
            { index: 0, cells: ['First'] },
            { index: 1, cells: ['Second'] },
          ],
          sheets: [{ name: 'Summary', rowCount: 2, columnCount: 1 }],
          selectedSheet: 'Summary',
          rowStart: 0,
          indexedRows: 2,
          totalRows: 2,
          sourceIndexedRows: 2,
          sourceTotalRows: 2,
          indexingComplete: true,
          loadingRows: false,
          warning: undefined,
          searchQuery: '',
          searchMatches: [],
          searchNextCursor: undefined,
          searching: false,
        },
      }),
    );

    const rows = root.querySelectorAll('.fm-structured-row');
    expect(rows[0]?.hasAttribute('data-row-stripe')).toBe(false);
    expect(rows[1]?.getAttribute('data-row-stripe')).toBe('alternate');
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
