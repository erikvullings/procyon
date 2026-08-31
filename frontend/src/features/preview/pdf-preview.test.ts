import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { renderPdfPageToCanvas } from './pdf-preview';

/** A fake pdf.js document whose `page.render()` resolves after a macrotask, returning a
 * cancellable `RenderTask`-like object - just enough to exercise `renderPdfPageToCanvas`'s
 * cancellation bookkeeping without pdf.js's real rendering pipeline. */
function fakePdfDocument(): {
  readonly pdfDocument: { getPage: (pageNumber: number) => Promise<unknown> };
  readonly renderCalls: number[];
  readonly cancelCalls: number[];
} {
  const renderCalls: number[] = [];
  const cancelCalls: number[] = [];
  let nextTaskId = 0;
  const page = {
    getViewport: ({ scale }: { scale: number }) => ({ width: 100 * scale, height: 200 * scale }),
    render: () => {
      const taskId = nextTaskId;
      nextTaskId += 1;
      renderCalls.push(taskId);
      let cancelled = false;
      const promise = new Promise((resolve, reject) => {
        setTimeout(() => {
          if (cancelled)
            reject(Object.assign(new Error('cancelled'), { name: 'RenderingCancelledException' }));
          else resolve(undefined);
        }, 0);
      });
      return {
        promise,
        cancel: () => {
          cancelled = true;
          cancelCalls.push(taskId);
        },
      };
    },
  };
  return { pdfDocument: { getPage: () => Promise.resolve(page) }, renderCalls, cancelCalls };
}

describe('renderPdfPageToCanvas', () => {
  let getContextSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    getContextSpy = vi
      .spyOn(HTMLCanvasElement.prototype, 'getContext')
      .mockReturnValue({} as CanvasRenderingContext2D);
  });

  afterEach(() => {
    getContextSpy.mockRestore();
  });

  it('supersedes an earlier call before it ever reaches page.render() when both start immediately back-to-back', async () => {
    const { pdfDocument, renderCalls, cancelCalls } = fakePdfDocument();
    const canvas = document.createElement('canvas');

    // Neither call is awaited before the second starts - mirrors the real `oncreate` +
    // immediate `ResizeObserver` firing race that used to throw "Cannot use the same canvas
    // during multiple render() operations." The earlier call is dropped entirely (never calls
    // `page.render()`) once the later one supersedes it.
    const first = renderPdfPageToCanvas(pdfDocument as never, 1, canvas, 200, 400);
    const second = renderPdfPageToCanvas(pdfDocument as never, 1, canvas, 300, 600);

    await expect(Promise.all([first, second])).resolves.toEqual([undefined, undefined]);
    expect(renderCalls).toEqual([0]);
    expect(cancelCalls).toEqual([]);
  });

  it('cancels an already-started render when a later call targets the same canvas', async () => {
    const { pdfDocument, renderCalls, cancelCalls } = fakePdfDocument();
    const canvas = document.createElement('canvas');

    const first = renderPdfPageToCanvas(pdfDocument as never, 1, canvas, 200, 400);
    await vi.waitFor(() => expect(renderCalls).toHaveLength(1));

    const second = renderPdfPageToCanvas(pdfDocument as never, 1, canvas, 300, 600);

    await expect(Promise.all([first, second])).resolves.toEqual([undefined, undefined]);
    expect(renderCalls).toEqual([0, 1]);
    expect(cancelCalls).toEqual([0]);
  });

  it('does not cancel a render targeting a different canvas', async () => {
    const { pdfDocument, cancelCalls } = fakePdfDocument();
    const canvasA = document.createElement('canvas');
    const canvasB = document.createElement('canvas');

    await Promise.all([
      renderPdfPageToCanvas(pdfDocument as never, 1, canvasA, 200, 400),
      renderPdfPageToCanvas(pdfDocument as never, 1, canvasB, 200, 400),
    ]);

    expect(cancelCalls).toEqual([]);
  });
});
