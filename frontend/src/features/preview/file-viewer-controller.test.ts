import { describe, expect, it, vi } from 'vitest';

import type { EntrySummary, PaneId } from '../../models';
import {
  createFileViewerController,
  type FileViewerClient,
  type FileViewerState,
  TEXT_WINDOW_BYTES,
  VIDEO_INLINE_SIZE_LIMIT_BYTES,
} from './file-viewer-controller';
import { loadPdfDocument } from './pdf-preview';

vi.mock('./pdf-preview', () => ({
  loadPdfDocument: vi.fn().mockResolvedValue({ numPages: 3 }),
}));

/** Builds a fake pdf.js document whose pages' text content is `pageText[pageNumber - 1]`. */
function fakePdfDocument(pageText: readonly string[]): {
  readonly numPages: number;
  readonly getPage: (
    pageNumber: number,
  ) => Promise<{ getTextContent: () => Promise<{ items: { str: string }[] }> }>;
} {
  return {
    numPages: pageText.length,
    getPage: (pageNumber: number) =>
      Promise.resolve({
        getTextContent: () => Promise.resolve({ items: [{ str: pageText[pageNumber - 1] ?? '' }] }),
      }),
  };
}

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

function setup(): {
  readonly client: FileViewerClient;
  readonly states: FileViewerState[];
} {
  return {
    client: {
      readFileRange: vi.fn(),
      openDocxPreview: vi.fn(),
      readDocxPreviewResource: vi.fn(),
      closeDocxPreview: vi.fn().mockResolvedValue(undefined),
      openPptxPreview: vi.fn(),
      readPptxPreviewResource: vi.fn(),
      closePptxPreview: vi.fn().mockResolvedValue(undefined),
      searchInFile: vi.fn(),
      listDirectory: vi.fn(),
      archiveSummary: vi.fn(),
      gitFileHistory: vi.fn().mockResolvedValue({ commits: [] }),
      openStructuredView: vi.fn(),
      getStructuredViewStatus: vi.fn(),
      updateStructuredView: vi.fn(),
      readStructuredRows: vi.fn(),
      readStructuredJsonWindow: vi.fn(),
      searchStructuredRows: vi.fn(),
      closeStructuredView: vi.fn().mockResolvedValue(undefined),
    },
    states: [],
  };
}

function textOf(state: FileViewerState | undefined): string | undefined {
  return state !== undefined && state.status === 'ready' && state.content.kind === 'text'
    ? state.content.text
    : undefined;
}

describe('file viewer controller', () => {
  it('loads one sanitized PPTX slide at a time and reuses paged navigation', async () => {
    const context = setup();
    vi.mocked(context.client.openPptxPreview).mockResolvedValue({
      sessionId: 'pptx-session',
      sourceRevision: 'r1',
      sourceBytes: 4096,
      slides: [
        { index: 0, title: 'Overview', markdown: '# Overview\n\nFirst slide' },
        {
          index: 1,
          title: 'Details',
          markdown: '# Details\n\n![Chart](pptx-resource:../media/chart.png)',
        },
      ],
      resources: [
        {
          resourceId: 'chart',
          source: '../media/chart.png',
          mediaType: 'image/png',
          byteLength: 4,
        },
      ],
      omittedFeatures: ['themes and precise geometry', 'charts'],
    });
    vi.mocked(context.client.readPptxPreviewResource).mockResolvedValue({
      data: [137, 80, 78, 71],
      mediaType: 'image/png',
    });

    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'briefing.pptx', extension: 'pptx' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        content: {
          kind: 'pptx',
          currentSlide: 0,
          slideCount: 2,
          currentSlideHtml: expect.stringContaining('First slide'),
        },
      }),
    );

    controller.nextPage();
    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        content: {
          currentSlide: 1,
          currentSlideHtml: expect.stringContaining('data:image/png;base64,'),
        },
      }),
    );
    controller.previousPage();
    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({ content: { currentSlide: 0 } }),
    );
    controller.dispose();
    expect(context.client.closePptxPreview).toHaveBeenCalledWith({ sessionId: 'pptx-session' });
  });

  it('searches across PPTX slides and navigates to the matching slide', async () => {
    const context = setup();
    vi.mocked(context.client.openPptxPreview).mockResolvedValue({
      sessionId: 'pptx-session',
      sourceRevision: 'r1',
      sourceBytes: 1024,
      slides: [
        { index: 0, title: 'Overview', markdown: '# Overview\n\nWelcome' },
        { index: 1, title: 'Decision', markdown: '# Decision\n\nApprove launch' },
      ],
      resources: [],
      omittedFeatures: [],
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'briefing.pptx', extension: 'pptx' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    controller.setSearchOptions({ query: 'Approve' });
    await controller.runSearch();

    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        content: { kind: 'pptx', currentSlide: 1 },
        search: { currentMatchIndex: 0, matches: [{ length: 7 }] },
      }),
    );
  });

  it('copies the currently displayed PPTX slide text', async () => {
    const context = setup();
    vi.mocked(context.client.openPptxPreview).mockResolvedValue({
      sessionId: 'pptx-session',
      sourceRevision: 'r1',
      sourceBytes: 1024,
      slides: [{ index: 0, title: 'Overview', markdown: '# Overview\n\nCopy this content' }],
      resources: [],
      omittedFeatures: [],
    });
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'briefing.pptx', extension: 'pptx' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        content: { currentSlideHtml: expect.stringContaining('Copy this content') },
      }),
    );

    await controller.copyContent();

    expect(writeText).toHaveBeenCalledWith(expect.stringContaining('Copy this content'));
  });

  it('retains external-open fallback when PPTX parsing exceeds a budget', async () => {
    const context = setup();
    vi.mocked(context.client.openPptxPreview).mockRejectedValue(
      new Error('PPTX content preview exceeds the slide-count budget'),
    );

    createFileViewerController({
      client: context.client,
      entry: entry({ name: 'oversized.pptx', extension: 'pptx' }),
      update: (state) => context.states.push(state),
    });

    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        content: {
          kind: 'pptxExternal',
          message: 'PPTX content preview exceeds the slide-count budget',
        },
      }),
    );
  });

  it('closes the PPTX session and falls back when a later slide cannot load', async () => {
    const context = setup();
    vi.mocked(context.client.openPptxPreview).mockResolvedValue({
      sessionId: 'pptx-session',
      sourceRevision: 'r1',
      sourceBytes: 1024,
      slides: [
        { index: 0, title: 'Overview', markdown: '# Overview' },
        {
          index: 1,
          title: 'Details',
          markdown: '![Chart](pptx-resource:../media/chart.png)',
        },
      ],
      resources: [
        {
          resourceId: 'chart',
          source: '../media/chart.png',
          mediaType: 'image/png',
          byteLength: 4,
        },
      ],
      omittedFeatures: [],
    });
    vi.mocked(context.client.readPptxPreviewResource).mockRejectedValue(
      new Error('The presentation changed'),
    );
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'briefing.pptx', extension: 'pptx' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({ content: { kind: 'pptx' } }),
    );

    controller.nextPage();

    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        content: { kind: 'pptxExternal', message: 'The presentation changed' },
      }),
    );
    expect(context.client.closePptxPreview).toHaveBeenCalledWith({ sessionId: 'pptx-session' });
  });

  it('loads and sanitizes a DOCX content preview with separately fetched images', async () => {
    const context = setup();
    vi.mocked(context.client.openDocxPreview).mockResolvedValue({
      sessionId: 'docx-session',
      sourceRevision: 'r1',
      sourceBytes: 2048,
      html: '<h1>Report</h1><p>Body</p><img src="media/image1.png"><script>evil()</script>',
      resources: [
        {
          resourceId: 'image-1',
          source: 'media/image1.png',
          mediaType: 'image/png',
          byteLength: 4,
        },
      ],
      omittedFeatures: ['exact pagination', 'tracked changes'],
    });
    vi.mocked(context.client.readDocxPreviewResource).mockResolvedValue({
      data: [137, 80, 78, 71],
      mediaType: 'image/png',
    });

    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'report.docx', extension: 'docx' }),
      update: (state) => context.states.push(state),
    });

    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        content: {
          kind: 'docx',
          html: expect.stringContaining('data:image/png;base64,'),
          plainText: expect.stringContaining('Report'),
          omittedFeatures: ['exact pagination', 'tracked changes'],
        },
      }),
    );
    expect((context.states.at(-1) as { content: { html: string } }).content.html).not.toContain(
      'script',
    );
    controller.dispose();
    expect(context.client.closeDocxPreview).toHaveBeenCalledWith({ sessionId: 'docx-session' });
  });

  it('shows an external-application fallback when bounded DOCX parsing fails', async () => {
    const context = setup();
    vi.mocked(context.client.openDocxPreview).mockRejectedValue(
      new Error('DOCX preview exceeds the expanded ZIP budget'),
    );

    createFileViewerController({
      client: context.client,
      entry: entry({ name: 'bomb.docx', extension: 'docx' }),
      update: (state) => context.states.push(state),
    });

    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        content: {
          kind: 'docxExternal',
          message: 'DOCX preview exceeds the expanded ZIP budget',
        },
      }),
    );
  });

  it('loads an archive summary for the selected archive file', async () => {
    const context = setup();
    vi.mocked(context.client.archiveSummary).mockResolvedValue({
      format: 'zip',
      fileCount: 3,
      directoryCount: 2,
      uncompressedSize: 4_096,
      compressedSize: 512,
    });
    createFileViewerController({
      client: context.client,
      entry: entry({
        name: 'bundle.zip',
        extension: 'zip',
        location: { providerId: 'local', uri: 'file:///tmp/bundle.zip' },
      }),
      update: (state) => context.states.push(state),
    });

    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));
    expect(context.client.archiveSummary).toHaveBeenCalledWith(
      { location: { providerId: 'local', uri: 'file:///tmp/bundle.zip' } },
      expect.any(AbortSignal),
    );
    expect(context.states.at(-1)).toMatchObject({
      content: {
        kind: 'archiveSummary',
        format: 'zip',
        fileCount: 3,
        directoryCount: 2,
        uncompressedSize: 4_096,
        compressedSize: 512,
      },
    });
  });

  it('searches and navigates sanitized DOCX content without a backend file search', async () => {
    const context = setup();
    vi.mocked(context.client.openDocxPreview).mockResolvedValue({
      sessionId: 'docx-session',
      sourceRevision: 'r1',
      sourceBytes: 32,
      html: '<h1>Report</h1><p>Another report</p>',
      resources: [],
      omittedFeatures: [],
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'report.docx', extension: 'docx' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    controller.setSearchOptions({ query: 'report' });
    await controller.runSearch();
    expect(context.states.at(-1)).toMatchObject({
      content: { kind: 'docx', html: expect.stringContaining('fm-docx-search-match-active') },
      search: { currentMatchIndex: 0 },
    });
    expect(
      (context.states.at(-1) as { search: { matches: readonly unknown[] } }).search.matches,
    ).toHaveLength(2);

    await controller.goToNextMatch();
    expect(context.states.at(-1)).toMatchObject({
      content: {
        kind: 'docx',
        html: expect.stringMatching(/Another <mark class="fm-docx-search-match-active">report/),
      },
      search: { currentMatchIndex: 1 },
    });
    expect(context.client.searchInFile).not.toHaveBeenCalled();

    controller.setSearchOptions({ query: '' });
    expect((context.states.at(-1) as { content: { html: string } }).content.html).not.toContain(
      '<mark',
    );
  });

  it('copies DOCX text and computes metadata from the complete bounded document', async () => {
    const context = setup();
    vi.mocked(context.client.openDocxPreview).mockResolvedValue({
      sessionId: 'docx-session',
      sourceRevision: 'r1',
      sourceBytes: 32,
      html: '<h1>Report</h1><p>First line<br>Second line</p>',
      resources: [],
      omittedFeatures: [],
    });
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'report.docx', extension: 'docx', size: 32 }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    await controller.copyContent();
    controller.toggleMetadataPanel();

    expect(writeText).toHaveBeenCalledWith('ReportFirst lineSecond line');
    expect(context.states.at(-1)).toMatchObject({
      metadataPanelOpen: true,
      metadata: {
        kind: 'text',
        sizeBytes: 32,
        characterCount: 27,
      },
    });
  });

  it('cancels archive summary loading when the viewer is disposed', async () => {
    const context = setup();
    let requestSignal: AbortSignal | undefined;
    vi.mocked(context.client.archiveSummary).mockImplementation((_request, signal) => {
      requestSignal = signal;
      return new Promise(() => undefined);
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'bundle.zip', extension: 'zip' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(requestSignal).toBeDefined());

    controller.dispose();

    expect(requestSignal?.aborted).toBe(true);
  });

  it('loads the first text window immediately on creation', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [104, 105],
      offset: 0,
      length: 2,
      eof: true,
    });
    createFileViewerController({
      client: context.client,
      entry: entry(),
      update: (state) => context.states.push(state),
    });

    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));
    expect(context.states[0]).toEqual({ status: 'loading', entry: entry() });
    expect(textOf(context.states.at(-1))).toBe('hi');
    expect(context.states.at(-1)).toMatchObject({
      content: { kind: 'text', windowOffset: 0, atStart: true, atEnd: true },
    });
  });

  it('publishes unsupported when the first chunk sniffs as binary', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [0, 1, 2],
      offset: 0,
      length: 3,
      eof: true,
      probablyBinary: true,
    });
    createFileViewerController({
      client: context.client,
      entry: entry(),
      update: (state) => context.states.push(state),
    });

    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('unsupported'));
  });

  it('loads a full image as a data URI regardless of size', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange)
      .mockResolvedValueOnce({ data: [1, 2, 3], offset: 0, length: 3, eof: false })
      .mockResolvedValueOnce({ data: [4, 5], offset: 3, length: 2, eof: true });
    createFileViewerController({
      client: context.client,
      entry: entry({ name: 'photo.png', extension: 'png' }),
      update: (state) => context.states.push(state),
    });

    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));
    expect(context.states.at(-1)).toMatchObject({
      content: { kind: 'image', zoom: 1, fitToContainer: true },
    });
  });

  it('loads a video below the inline limit as a MIME-specific data URI', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [1, 2, 3],
      offset: 0,
      length: 3,
      eof: true,
    });
    createFileViewerController({
      client: context.client,
      entry: entry({ name: 'clip.mp4', extension: 'mp4', size: 3 }),
      update: (state) => context.states.push(state),
    });

    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));
    expect(context.states.at(-1)).toMatchObject({
      content: { kind: 'video', dataUri: 'data:video/mp4;base64,AQID' },
    });
  });

  it('keeps a video above the inline limit external without reading file bytes', async () => {
    const context = setup();
    createFileViewerController({
      client: context.client,
      entry: entry({
        name: 'movie.webm',
        extension: 'webm',
        size: VIDEO_INLINE_SIZE_LIMIT_BYTES + 1,
      }),
      update: (state) => context.states.push(state),
    });

    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));
    expect(context.states.at(-1)).toMatchObject({ content: { kind: 'videoExternal' } });
    expect(context.client.readFileRange).not.toHaveBeenCalled();
  });

  it('keeps MKV videos external regardless of size', async () => {
    const context = setup();
    createFileViewerController({
      client: context.client,
      entry: entry({ name: 'clip.mkv', extension: 'mkv', size: 3 }),
      update: (state) => context.states.push(state),
    });

    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));
    expect(context.states.at(-1)).toMatchObject({ content: { kind: 'videoExternal' } });
    expect(context.client.readFileRange).not.toHaveBeenCalled();
  });

  it('switches to external playback when stale metadata understates the video size', async () => {
    const context = setup();
    const fullChunk = new Array<number>(1024 * 1024).fill(1);
    vi.mocked(context.client.readFileRange)
      .mockResolvedValueOnce({
        data: fullChunk,
        offset: 0,
        length: fullChunk.length,
        eof: false,
      })
      .mockResolvedValueOnce({
        data: fullChunk,
        offset: fullChunk.length,
        length: fullChunk.length,
        eof: false,
      })
      .mockResolvedValueOnce({
        data: [1],
        offset: VIDEO_INLINE_SIZE_LIMIT_BYTES,
        length: 1,
        eof: false,
      });
    createFileViewerController({
      client: context.client,
      entry: entry({ name: 'growing.mp4', extension: 'mp4', size: 3 }),
      update: (state) => context.states.push(state),
    });

    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({ content: { kind: 'videoExternal' } }),
    );
    expect(context.client.readFileRange).toHaveBeenLastCalledWith(
      {
        location: entry().location,
        offset: VIDEO_INLINE_SIZE_LIMIT_BYTES,
        length: 1,
      },
      expect.any(AbortSignal),
    );
  });

  it('replaces the bounded window via loadMore, without re-fetching from the start', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValueOnce({
      data: [104],
      offset: 0,
      length: 1,
      eof: false,
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry(),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    vi.mocked(context.client.readFileRange).mockResolvedValueOnce({
      data: [105],
      offset: 1,
      length: 1,
      eof: true,
    });
    await controller.loadMore();

    expect(context.client.readFileRange).toHaveBeenLastCalledWith(
      { location: entry().location, offset: 1, length: TEXT_WINDOW_BYTES },
      expect.any(AbortSignal),
    );
    expect(textOf(context.states.at(-1))).toBe('i');
    expect(context.states.at(-1)).toMatchObject({
      content: { windowOffset: 1, windowEnd: 2, atStart: false, atEnd: true },
    });
  });

  it('does not loadMore past the end of the file', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [104],
      offset: 0,
      length: 1,
      eof: true,
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry(),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    await controller.loadMore();

    expect(context.client.readFileRange).toHaveBeenCalledTimes(1);
  });

  it('opens CSV through the structured session and keeps only one bounded row page', async () => {
    const context = setup();
    vi.mocked(context.client.openStructuredView).mockResolvedValue({
      sessionId: 'csv-session',
      kind: 'table',
      sourceRevision: 'r1',
      sourceBytes: 8_000_000_000,
      randomAccess: true,
      delimiter: ';',
      headerMode: 'firstRow',
      headers: ['name', 'notes'],
      rows: [{ index: 0, cells: ['Ada', 'one\ntwo'] }],
      indexedBytes: 65_536,
      indexedRows: 1,
      indexingComplete: false,
    });
    vi.mocked(context.client.getStructuredViewStatus).mockResolvedValue({
      indexedBytes: 65_536,
      indexedRows: 1,
      indexingComplete: false,
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'report.csv', extension: 'csv' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    expect(context.client.readFileRange).not.toHaveBeenCalled();
    expect(context.states.at(-1)).toMatchObject({
      content: {
        kind: 'structuredTable',
        delimiter: ';',
        headers: ['name', 'notes'],
        rows: [{ index: 0, cells: ['Ada', 'one\ntwo'] }],
      },
    });

    vi.mocked(context.client.readStructuredRows).mockResolvedValue({
      rows: Array.from({ length: 200 }, (_, offset) => ({
        index: 10_000 + offset,
        cells: [`row-${offset}`],
      })),
      indexedRows: 20_000,
      indexingComplete: false,
    });
    await controller.loadStructuredRows(10_000);
    const latest = context.states.at(-1);
    expect(
      latest?.status === 'ready' && latest.content.kind === 'structuredTable'
        ? latest.content.rows.length
        : 0,
    ).toBe(200);
    controller.dispose();
    expect(context.client.closeStructuredView).toHaveBeenCalledWith({ sessionId: 'csv-session' });
  });

  it('loads multi-GB JSON as one bounded token window', async () => {
    const context = setup();
    vi.mocked(context.client.openStructuredView).mockResolvedValue({
      sessionId: 'json-session',
      kind: 'jsonText',
      sourceRevision: 'r1',
      sourceBytes: 4_000_000_000,
      randomAccess: true,
      headerMode: 'none',
      headers: [],
      rows: [],
      indexedBytes: 65_536,
      indexedRows: 0,
      indexingComplete: false,
    });
    vi.mocked(context.client.readStructuredJsonWindow).mockResolvedValue({
      data: Array.from(new TextEncoder().encode('{"city":"Zürich"}')),
      offset: 0,
      eof: false,
      tokens: [{ kind: 'property', start: 1, length: 6 }],
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'huge.json', extension: 'json' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));
    expect(context.states.at(-1)).toMatchObject({
      content: { kind: 'structuredJson', sourceBytes: 4_000_000_000, atEnd: false },
    });
    expect(context.client.readStructuredJsonWindow).toHaveBeenCalledWith(
      { sessionId: 'json-session', offset: 0, length: TEXT_WINDOW_BYTES },
      expect.any(AbortSignal),
    );
    controller.dispose();
  });

  it('opens .jsonl through the bounded highlighted JSON viewer', async () => {
    const context = setup();
    vi.mocked(context.client.openStructuredView).mockResolvedValue({
      sessionId: 'jsonl-session',
      kind: 'jsonText',
      sourceRevision: 'r1',
      sourceBytes: 900_000,
      randomAccess: true,
      headerMode: 'none',
      headers: [],
      rows: [],
      indexedBytes: 65_536,
      indexedRows: 0,
      indexingComplete: false,
    });
    vi.mocked(context.client.readStructuredJsonWindow).mockResolvedValue({
      data: Array.from(new TextEncoder().encode('{"message":"a very long record"}\n')),
      offset: 0,
      eof: true,
      tokens: [
        { kind: 'property', start: 1, length: 9 },
        { kind: 'string', start: 11, length: 20 },
      ],
    });

    createFileViewerController({
      client: context.client,
      entry: entry({ name: 'events.jsonl', extension: 'jsonl' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    expect(context.client.openStructuredView).toHaveBeenCalledWith(
      expect.objectContaining({ format: 'json' }),
      expect.any(AbortSignal),
    );
    expect(context.states.at(-1)).toMatchObject({ content: { kind: 'structuredJson' } });
  });

  it('uses structured search matches as the active table rows and restores rows when cleared', async () => {
    const context = setup();
    vi.mocked(context.client.openStructuredView).mockResolvedValue({
      sessionId: 'csv-session',
      kind: 'table',
      sourceRevision: 'r1',
      sourceBytes: 800_000,
      randomAccess: true,
      delimiter: ',',
      headerMode: 'firstRow',
      headers: ['name'],
      rows: [
        { index: 0, cells: ['Ada'] },
        { index: 1, cells: ['Grace'] },
      ],
      indexedBytes: 800_000,
      indexedRows: 2,
      totalRows: 2,
      indexingComplete: true,
    });
    vi.mocked(context.client.searchStructuredRows).mockResolvedValue({
      matches: [{ index: 1, cells: ['Grace'] }],
      indexingComplete: true,
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'people.csv', extension: 'csv' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    await controller.searchStructuredRows('Grace');
    expect(context.states.at(-1)).toMatchObject({
      content: { rows: [{ index: 0, cells: ['Grace'] }], indexedRows: 1 },
    });

    vi.mocked(context.client.readStructuredRows).mockResolvedValue({
      rows: [
        { index: 0, cells: ['Ada'] },
        { index: 1, cells: ['Grace'] },
      ],
      indexedRows: 2,
      totalRows: 2,
      indexingComplete: true,
    });
    await controller.searchStructuredRows('');
    expect(context.states.at(-1)).toMatchObject({
      content: {
        rows: [
          { index: 0, cells: ['Ada'] },
          { index: 1, cells: ['Grace'] },
        ],
        indexedRows: 2,
      },
    });
  });

  it('sorts a fully indexed bounded CSV by a selected column', async () => {
    const context = setup();
    vi.mocked(context.client.openStructuredView).mockResolvedValue({
      sessionId: 'csv-session',
      kind: 'table',
      sourceRevision: 'r1',
      sourceBytes: 800_000,
      randomAccess: true,
      delimiter: ',',
      headerMode: 'firstRow',
      headers: ['name'],
      rows: [
        { index: 0, cells: ['Grace'] },
        { index: 1, cells: ['Ada'] },
      ],
      indexedBytes: 800_000,
      indexedRows: 2,
      totalRows: 2,
      indexingComplete: true,
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'people.csv', extension: 'csv' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    await controller.sortStructuredRows(0);
    expect(context.states.at(-1)).toMatchObject({
      content: {
        rows: [
          { index: 0, cells: ['Ada'] },
          { index: 1, cells: ['Grace'] },
        ],
        sortColumn: 0,
        sortDirection: 'ascending',
      },
    });
  });

  it('runs a search and jumps to the first match', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValueOnce({
      data: Array.from('start\n', (char) => char.charCodeAt(0)),
      offset: 0,
      length: 6,
      eof: false,
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry(),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    vi.mocked(context.client.searchInFile).mockResolvedValue({
      matches: [{ offset: 40_000, length: 3, lineNumber: 1200 }],
      truncated: false,
    });
    const expectedWindowOffset = 40_000 - TEXT_WINDOW_BYTES / 2;
    vi.mocked(context.client.readFileRange).mockResolvedValueOnce({
      data: Array.from('...cat...', (char) => char.charCodeAt(0)),
      offset: expectedWindowOffset,
      length: 9,
      eof: false,
    });
    controller.setSearchOptions({ query: 'cat' });
    await controller.runSearch();

    expect(context.client.searchInFile).toHaveBeenCalledWith(
      {
        location: entry().location,
        query: 'cat',
        regex: false,
        caseSensitive: false,
        wholeWord: false,
      },
      expect.any(AbortSignal),
    );
    const last = context.states.at(-1);
    expect(last).toMatchObject({
      status: 'ready',
      content: { windowOffset: expectedWindowOffset, highlightOffset: 9, highlightLength: 0 },
      search: {
        matches: [{ offset: 40_000, length: 3, lineNumber: 1200 }],
        currentMatchIndex: 0,
      },
    });
  });

  it('converts the match byte offset to a character offset when multi-byte text precedes it', async () => {
    // "café — cat": byte offset of "cat" is 10 (é=2 bytes, — =3 bytes), but its character offset
    // is only 7 - using the raw byte offset directly (the bug) would highlight 3 characters late.
    const fileText = 'café — cat';
    const fileBytes = new TextEncoder().encode(fileText);
    const matchOffset = fileBytes.indexOf('c'.charCodeAt(0), 8); // byte offset of "cat"
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValueOnce({
      data: Array.from(fileBytes),
      offset: 0,
      length: fileBytes.length,
      eof: true,
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry(),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    vi.mocked(context.client.searchInFile).mockResolvedValue({
      matches: [{ offset: matchOffset, length: 3, lineNumber: 1 }],
      truncated: false,
    });
    vi.mocked(context.client.readFileRange).mockResolvedValueOnce({
      data: Array.from(fileBytes),
      offset: 0,
      length: fileBytes.length,
      eof: true,
    });
    controller.setSearchOptions({ query: 'cat' });
    await controller.runSearch();

    const last = context.states.at(-1);
    expect(last).toMatchObject({
      status: 'ready',
      content: {
        windowOffset: 0,
        highlightOffset: fileText.indexOf('cat'),
        highlightLength: 3,
      },
    });
  });

  it('runs a debounced search automatically as the query is edited, without requiring runSearch', async () => {
    vi.useFakeTimers();
    try {
      const context = setup();
      vi.mocked(context.client.readFileRange).mockResolvedValueOnce({
        data: Array.from('start\n', (char) => char.charCodeAt(0)),
        offset: 0,
        length: 6,
        eof: false,
      });
      const controller = createFileViewerController({
        client: context.client,
        entry: entry(),
        update: (state) => context.states.push(state),
      });
      await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'), {
        timeout: 1000,
        interval: 1,
      });

      vi.mocked(context.client.searchInFile).mockResolvedValue({ matches: [], truncated: false });
      controller.setSearchOptions({ query: 'cat' });
      expect(context.client.searchInFile).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(200);
      expect(context.client.searchInFile).toHaveBeenCalledTimes(1);
      expect(context.client.searchInFile).toHaveBeenCalledWith(
        {
          location: entry().location,
          query: 'cat',
          regex: false,
          caseSensitive: false,
          wholeWord: false,
        },
        expect.any(AbortSignal),
      );
    } finally {
      vi.useRealTimers();
    }
  });

  it('clears stale matches immediately once the query is emptied, without waiting on the debounce', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValueOnce({
      data: Array.from('start\n', (char) => char.charCodeAt(0)),
      offset: 0,
      length: 6,
      eof: false,
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry(),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    controller.setSearchOptions({
      query: '',
    });
    const last = context.states.at(-1);
    expect(last).toMatchObject({ search: { query: '', matches: [] } });
  });

  it('wraps around when navigating past the last match', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValueOnce({
      data: [1],
      offset: 0,
      length: 1,
      eof: true,
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry(),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    vi.mocked(context.client.searchInFile).mockResolvedValue({
      matches: [
        { offset: 10, length: 1, lineNumber: 1 },
        { offset: 20, length: 1, lineNumber: 2 },
      ],
      truncated: false,
    });
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [1],
      offset: 0,
      length: 1,
      eof: true,
    });
    controller.setSearchOptions({ query: 'x' });
    await controller.runSearch();
    expect(context.states.at(-1)).toMatchObject({ search: { currentMatchIndex: 0 } });

    await controller.goToNextMatch();
    expect(context.states.at(-1)).toMatchObject({ search: { currentMatchIndex: 1 } });

    await controller.goToNextMatch();
    expect(context.states.at(-1)).toMatchObject({ search: { currentMatchIndex: 0 } });

    await controller.goToPreviousMatch();
    expect(context.states.at(-1)).toMatchObject({ search: { currentMatchIndex: 1 } });
  });

  it('zooms an image in, out, and resets to fit-to-container', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [1],
      offset: 0,
      length: 1,
      eof: true,
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'photo.png', extension: 'png' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    controller.zoomIn();
    expect(context.states.at(-1)).toMatchObject({ content: { fitToContainer: false, zoom: 1.25 } });

    controller.zoomOut();
    expect(context.states.at(-1)).toMatchObject({ content: { zoom: 1 } });

    controller.zoomIn();
    controller.resetZoom();
    expect(context.states.at(-1)).toMatchObject({ content: { fitToContainer: true, zoom: 1 } });
  });

  it('publishes an error state when the initial load rejects', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockRejectedValue(new Error('boom'));
    createFileViewerController({
      client: context.client,
      entry: entry(),
      update: (state) => context.states.push(state),
    });

    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('error'));
    expect(context.states.at(-1)).toEqual({ status: 'error', entry: entry(), message: 'boom' });
  });

  it('copies the loaded text window to the clipboard', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [104, 105],
      offset: 0,
      length: 2,
      eof: true,
    });
    const writeText = vi.fn().mockResolvedValue(undefined);
    vi.stubGlobal('navigator', { clipboard: { writeText } });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry(),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    await controller.copyContent();

    expect(writeText).toHaveBeenCalledWith('hi');
    vi.unstubAllGlobals();
  });

  it('copies the loaded image to the clipboard as image bytes', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [1, 2, 3],
      offset: 0,
      length: 3,
      eof: true,
    });
    const write = vi.fn().mockResolvedValue(undefined);
    class FakeClipboardItem {
      constructor(readonly items: Record<string, Blob>) {}
    }
    vi.stubGlobal('navigator', { clipboard: { write } });
    vi.stubGlobal('ClipboardItem', FakeClipboardItem);
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'photo.png', extension: 'png' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    await controller.copyContent();

    expect(write).toHaveBeenCalledTimes(1);
    vi.unstubAllGlobals();
  });

  it('computes text metadata (size/lines/characters/language) when the panel is opened', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [104, 105, 10, 104, 105],
      offset: 0,
      length: 5,
      eof: true,
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ extension: 'ts', name: 'report.ts', size: 5 }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    controller.toggleMetadataPanel();

    const state = context.states.at(-1);
    expect(state).toMatchObject({
      metadataPanelOpen: true,
      metadata: {
        kind: 'text',
        sizeBytes: 5,
        lineCount: 2,
        characterCount: 5,
        language: 'typescript',
      },
    });
  });

  it('toggles the metadata panel closed again without discarding the computed metadata', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [104],
      offset: 0,
      length: 1,
      eof: true,
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry(),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    controller.toggleMetadataPanel();
    controller.toggleMetadataPanel();

    const state = context.states.at(-1);
    expect(state).toMatchObject({ metadataPanelOpen: false });
    expect((state as { metadata?: unknown }).metadata).toBeDefined();
  });

  it('fetches git history when the metadata panel is opened', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [104],
      offset: 0,
      length: 1,
      eof: true,
    });
    vi.mocked(context.client.gitFileHistory).mockResolvedValue({
      commits: [
        {
          commitId: 'a1b2c3d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0',
          shortId: 'a1b2c3d',
          authorName: 'Ada Lovelace',
          authorEmail: 'ada@example.com',
          committedAt: '2026-01-15T09:30:00Z',
          summary: 'fix(app): handle empty selection',
        },
      ],
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry(),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    controller.toggleMetadataPanel();
    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        gitHistory: [{ summary: 'fix(app): handle empty selection' }],
      }),
    );

    expect(context.client.gitFileHistory).toHaveBeenCalledWith({
      location: entry().location,
    });
  });

  it('resolves git history to an empty list when the file has none, rather than rejecting', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [104],
      offset: 0,
      length: 1,
      eof: true,
    });
    vi.mocked(context.client.gitFileHistory).mockRejectedValue(new Error('network error'));
    const controller = createFileViewerController({
      client: context.client,
      entry: entry(),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    controller.toggleMetadataPanel();
    await vi.waitFor(() => expect(context.states.at(-1)).toMatchObject({ gitHistory: [] }));
  });

  it('loads a PDF and exposes page count/current page, navigable via next/previousPage', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [1, 2, 3],
      offset: 0,
      length: 3,
      eof: true,
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'report.pdf', extension: 'pdf' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));
    expect(context.states.at(-1)).toMatchObject({
      content: { kind: 'pdf', pageCount: 3, currentPage: 1 },
    });

    controller.nextPage();
    expect(context.states.at(-1)).toMatchObject({ content: { currentPage: 2 } });
    controller.previousPage();
    controller.previousPage();
    expect(context.states.at(-1)).toMatchObject({ content: { currentPage: 1 } });

    controller.nextPage();
    controller.nextPage();
    controller.nextPage();
    expect(context.states.at(-1)).toMatchObject({ content: { currentPage: 3 } });
  });

  it('finds matching PDF pages via simple text search and jumps between them', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [1, 2, 3],
      offset: 0,
      length: 3,
      eof: true,
    });
    vi.mocked(loadPdfDocument).mockResolvedValueOnce(
      fakePdfDocument(['apple pie', 'banana bread', 'apple crumble']) as never,
    );
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'report.pdf', extension: 'pdf' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    controller.setPdfSearchQuery('apple');
    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        pdfSearch: { matches: [1, 3], currentMatchIndex: 0 },
        content: { currentPage: 1 },
      }),
    );

    controller.goToNextPdfMatch();
    expect(context.states.at(-1)).toMatchObject({
      content: { currentPage: 3 },
      pdfSearch: { currentMatchIndex: 1 },
    });

    controller.goToPreviousPdfMatch();
    expect(context.states.at(-1)).toMatchObject({
      content: { currentPage: 1 },
      pdfSearch: { currentMatchIndex: 0 },
    });
  });

  it('clears PDF search matches when the query is emptied', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [1, 2, 3],
      offset: 0,
      length: 3,
      eof: true,
    });
    vi.mocked(loadPdfDocument).mockResolvedValueOnce(fakePdfDocument(['apple', 'banana']) as never);
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'report.pdf', extension: 'pdf' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('ready'));

    controller.setPdfSearchQuery('apple');
    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({ pdfSearch: { matches: [1] } }),
    );

    controller.setPdfSearchQuery('');
    expect(context.states.at(-1)).toMatchObject({ pdfSearch: { query: '', matches: [] } });
  });

  it('loads an EPUB, parsing container.xml/OPF and rendering the first chapter', async () => {
    const context = setup();
    const containerXml =
      '<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>';
    const opfXml =
      '<package><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Book</dc:title></metadata>' +
      '<manifest>' +
      '<item id="c1" href="c1.xhtml" media-type="application/xhtml+xml"/>' +
      '<item id="c2" href="c2.xhtml" media-type="application/xhtml+xml"/>' +
      '<item id="cover" href="images/cover.png" media-type="image/png"/>' +
      '</manifest>' +
      '<spine><itemref idref="c1"/><itemref idref="c2"/></spine>' +
      '</package>';
    const chapterHtml: Record<string, string> = {
      'archive:///tmp/report.txt!/OEBPS/c1.xhtml':
        '<p>Chapter one</p><img src="images/cover.png" alt="Cover">',
      'archive:///tmp/report.txt!/OEBPS/c2.xhtml': '<p>Chapter two</p>',
    };
    vi.mocked(context.client.readFileRange).mockImplementation(async (request) => {
      const uri = request.location.uri;
      if (uri.endsWith('OEBPS/images/cover.png')) {
        return { data: [137, 80, 78, 71], offset: 0, length: 4, eof: true };
      }
      const text = uri.endsWith('META-INF/container.xml')
        ? containerXml
        : uri.endsWith('OEBPS/content.opf')
          ? opfXml
          : (chapterHtml[uri] ?? '');
      const data = Array.from(new TextEncoder().encode(text));
      return { data, offset: 0, length: data.length, eof: true };
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'book.epub', extension: 'epub' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        content: { kind: 'epub', currentChapterHtml: expect.any(String) },
      }),
    );
    expect(context.states.at(-1)).toMatchObject({
      content: { title: 'Book', chapterCount: 2, currentChapter: 0, loadingChapter: false },
    });
    expect(
      (context.states.at(-1) as { content: { currentChapterHtml: string } }).content
        .currentChapterHtml,
    ).toContain('Chapter one');
    expect(
      (context.states.at(-1) as { content: { currentChapterHtml: string } }).content
        .currentChapterHtml,
    ).toContain('src="data:image/png;base64,iVBORw=="');
    expect(context.client.readFileRange).toHaveBeenCalledWith(
      {
        location: {
          providerId: 'archive',
          uri: 'archive:///tmp/report.txt!/OEBPS/images/cover.png',
        },
        offset: 0,
        length: 1024 * 1024,
      },
      expect.any(AbortSignal),
    );

    controller.nextPage();
    await vi.waitFor(() =>
      expect(
        (context.states.at(-1) as { content: { currentChapterHtml?: string } }).content
          .currentChapterHtml,
      ).toContain('Chapter two'),
    );
    expect(context.states.at(-1)).toMatchObject({ content: { currentChapter: 1 } });
  });

  it('repairs a malformed EPUB spine when numbered TOC labels confirm the order', async () => {
    const context = setup();
    const containerXml =
      '<container><rootfiles><rootfile full-path="content.opf"/></rootfiles></container>';
    const opfXml =
      '<package><manifest>' +
      '<item id="c11" href="chapter_11.xhtml" media-type="application/xhtml+xml"/>' +
      '<item id="c10" href="chapter_10.xhtml" media-type="application/xhtml+xml"/>' +
      '<item id="c12" href="chapter_12.xhtml" media-type="application/xhtml+xml"/>' +
      '<item id="c5" href="chapter_05.xhtml" media-type="application/xhtml+xml"/>' +
      '<item id="toc" href="toc.ncx" media-type="application/x-dtbncx+xml"/>' +
      '</manifest><spine toc="toc">' +
      '<itemref idref="c11"/><itemref idref="c10"/><itemref idref="c12"/><itemref idref="c5"/>' +
      '</spine></package>';
    const tocXml = `<ncx><navMap>
      <navPoint><navLabel><text>Chapter Eleven</text></navLabel><content src="chapter_11.xhtml"/></navPoint>
      <navPoint><navLabel><text>Chapter Ten</text></navLabel><content src="chapter_10.xhtml"/></navPoint>
      <navPoint><navLabel><text>Chapter Twelve</text></navLabel><content src="chapter_12.xhtml"/></navPoint>
      <navPoint><navLabel><text>Chapter Five</text></navLabel><content src="chapter_05.xhtml"/></navPoint>
    </navMap></ncx>`;
    vi.mocked(context.client.readFileRange).mockImplementation(async (request) => {
      const uri = request.location.uri;
      const text = uri.endsWith('META-INF/container.xml')
        ? containerXml
        : uri.endsWith('content.opf')
          ? opfXml
          : uri.endsWith('toc.ncx')
            ? tocXml
            : uri.endsWith('chapter_05.xhtml')
              ? '<h1>Chapter Five</h1>'
              : '';
      const data = Array.from(new TextEncoder().encode(text));
      return { data, offset: 0, length: data.length, eof: true };
    });

    createFileViewerController({
      client: context.client,
      entry: entry({ name: 'book.epub', extension: 'epub' }),
      update: (state) => context.states.push(state),
    });

    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        content: {
          kind: 'epub',
          chapterCount: 4,
          currentChapter: 0,
          currentChapterHtml: expect.stringContaining('Chapter Five'),
        },
      }),
    );
  });

  it('shows an error for an EPUB without a locatable OPF package document', async () => {
    const context = setup();
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: Array.from(new TextEncoder().encode('<container><rootfiles/></container>')),
      offset: 0,
      length: 10,
      eof: true,
    });
    createFileViewerController({
      client: context.client,
      entry: entry({ name: 'book.epub', extension: 'epub' }),
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('error'));
  });

  it('loads a comic archive, fetching the first page and paginating on demand', async () => {
    const context = setup();
    const paneId = 'pane-a' as PaneId;
    vi.mocked(context.client.listDirectory).mockResolvedValue({
      paneId,
      requestId: 'req-1',
      revision: 1,
      location: { providerId: 'archive', uri: 'archive:///tmp/book.cbz!/' },
      writable: false,
      hasMore: false,
      loadingState: { type: 'loaded' },
      entries: [
        entry({
          id: 'page-2',
          name: 'page02.jpg',
          extension: 'jpg',
          location: { providerId: 'archive', uri: 'archive:///tmp/book.cbz!/page02.jpg' },
        }),
        entry({
          id: 'page-1',
          name: 'page01.jpg',
          extension: 'jpg',
          location: { providerId: 'archive', uri: 'archive:///tmp/book.cbz!/page01.jpg' },
        }),
      ],
    });
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [1, 2, 3],
      offset: 0,
      length: 3,
      eof: true,
    });
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'book.cbz', extension: 'cbz' }),
      workspaceId: 'workspace-1',
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        content: { kind: 'comic', currentPageDataUri: expect.any(String) },
      }),
    );
    expect(context.states.at(-1)).toMatchObject({
      content: { pageCount: 2, currentPage: 0, loadingPage: false },
    });

    controller.nextPage();
    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        content: { currentPage: 1, loadingPage: false },
      }),
    );
  });

  it('finds comic pages nested inside a single wrapper folder at the archive root', async () => {
    // Some CBR/CBZ archives (e.g. "one folder per volume" scans) wrap their pages in a top-level
    // directory instead of placing them at the archive root - the root listing alone finds no
    // images, so the controller must descend into subdirectories before giving up.
    const context = setup();
    vi.mocked(context.client.listDirectory).mockImplementation(async (request) => {
      const atRoot = request.location.uri === 'archive:///tmp/book.cbr!/';
      return {
        paneId: 'pane-a' as PaneId,
        requestId: 'req-1',
        revision: 1,
        location: request.location,
        writable: false,
        hasMore: false,
        loadingState: { type: 'loaded' },
        entries: atRoot
          ? [
              entry({
                id: 'wrapper',
                name: 'Volume 1',
                kind: 'directory',
                location: { providerId: 'archive', uri: 'archive:///tmp/book.cbr!/Volume 1' },
              }),
            ]
          : [
              entry({
                id: 'page-1',
                name: 'page01.jpg',
                extension: 'jpg',
                location: {
                  providerId: 'archive',
                  uri: 'archive:///tmp/book.cbr!/Volume 1/page01.jpg',
                },
              }),
            ],
      };
    });
    vi.mocked(context.client.readFileRange).mockResolvedValue({
      data: [1, 2, 3],
      offset: 0,
      length: 3,
      eof: true,
    });
    createFileViewerController({
      client: context.client,
      entry: entry({ name: 'book.cbr', extension: 'cbr' }),
      workspaceId: 'workspace-1',
      update: (state) => context.states.push(state),
    });
    await vi.waitFor(() =>
      expect(context.states.at(-1)).toMatchObject({
        content: { kind: 'comic', pageCount: 1, currentPageDataUri: expect.any(String) },
      }),
    );
  });

  it('shows an error for a comic opened without workspace context', async () => {
    const context = setup();
    const controller = createFileViewerController({
      client: context.client,
      entry: entry({ name: 'book.cbz', extension: 'cbz' }),
      update: (state) => context.states.push(state),
    });
    void controller;
    await vi.waitFor(() => expect(context.states.at(-1)?.status).toBe('error'));
  });

  it('stops publishing after dispose', async () => {
    const context = setup();
    const pending = new Promise<never>(() => undefined);
    vi.mocked(context.client.readFileRange).mockReturnValue(pending);
    const controller = createFileViewerController({
      client: context.client,
      entry: entry(),
      update: (state) => context.states.push(state),
    });
    const countBeforeDispose = context.states.length;
    controller.dispose();

    await controller.loadMore();

    expect(context.states.length).toBe(countBeforeDispose);
  });
});
