// The "legacy" build (rather than the default `pdfjs-dist` entry) is used deliberately: the
// default build assumes `DOMMatrix`/`Path2D` etc. are always present, which is true in real
// browsers/Tauri's webview but not in the vitest/jsdom test environment - the legacy build detects
// and polyfills for that case, so it works in both without needing a separate test-only mock.
import {
  GlobalWorkerOptions,
  getDocument,
  type PDFDocumentProxy,
  type PDFPageProxy,
  TextLayer,
} from 'pdfjs-dist/legacy/build/pdf.mjs';

// pdf.js renders/parses on a dedicated worker; point it at the bundler-emitted worker asset
// rather than the default same-origin lookup, which fails under Vite's dev/build asset hashing.
GlobalWorkerOptions.workerSrc = new URL(
  'pdfjs-dist/legacy/build/pdf.worker.mjs',
  import.meta.url,
).toString();

// Same reasoning as `workerSrc`: without an explicit, bundler-resolved `wasmUrl`, pdf.js's
// default same-origin "wasm" lookup 404s, so its WASM image codecs (OpenJPEG for JPX/JPEG2000,
// JBIG2) silently fail to initialize and any page using them renders blank instead of throwing.
// pdf.js fetches `${wasmUrl}${filename}` as a plain string concatenation (no per-file hook), so
// `wasmUrl` must be the *directory* containing openjpeg.wasm/jbig2.wasm/etc. Resolve one known
// file the same way `workerSrc` does above, then strip its filename back off to get that directory
// - `new URL('pdfjs-dist/wasm/', import.meta.url)` isn't usable directly because Vite's asset
// resolution only rewrites `new URL()` calls that target an actual file, not a bare directory.
const resolvedOpenjpegWasmUrl = new URL(
  'pdfjs-dist/wasm/openjpeg.wasm',
  import.meta.url,
).toString();
const wasmUrl = resolvedOpenjpegWasmUrl.slice(0, resolvedOpenjpegWasmUrl.lastIndexOf('/') + 1);

export type { PDFDocumentProxy };

/** Parses a PDF from its full bytes. Never executes embedded JavaScript/actions (task 0071's
 * "previewed files are never executed" requirement) - pdf.js only parses/rasterizes; it does not
 * run embedded AcroForm/JS unless a caller explicitly wires up its optional JS sandbox, which this
 * app never does. */
export async function loadPdfDocument(bytes: Uint8Array): Promise<PDFDocumentProxy> {
  return getDocument({ data: bytes, wasmUrl }).promise;
}

/** Wraps matching fragments in an already-positioned pdf.js text layer. */
export function highlightPdfTextLayer(
  textDivs: readonly HTMLElement[],
  textItems: readonly string[],
  expression: RegExp,
  activeOccurrenceIndex: number | undefined,
): void {
  const itemStarts: number[] = [];
  let searchableText = '';
  for (const text of textItems) {
    itemStarts.push(searchableText.length);
    searchableText += text;
  }

  const rangesByItem = textItems.map(
    () => [] as { start: number; end: number; occurrenceIndex: number }[],
  );
  const globalExpression = new RegExp(
    expression.source,
    expression.flags.includes('g') ? expression.flags : `${expression.flags}g`,
  );
  let occurrenceIndex = 0;
  for (const match of searchableText.matchAll(globalExpression)) {
    if (match[0].length === 0) continue;
    const matchStart = match.index;
    const matchEnd = matchStart + match[0].length;
    for (let itemIndex = 0; itemIndex < textItems.length; itemIndex += 1) {
      const itemStart = itemStarts[itemIndex] ?? 0;
      const itemEnd = itemStart + (textItems[itemIndex]?.length ?? 0);
      const overlapStart = Math.max(matchStart, itemStart);
      const overlapEnd = Math.min(matchEnd, itemEnd);
      if (overlapStart < overlapEnd) {
        rangesByItem[itemIndex]?.push({
          start: overlapStart - itemStart,
          end: overlapEnd - itemStart,
          occurrenceIndex,
        });
      }
    }
    occurrenceIndex += 1;
  }

  for (let itemIndex = 0; itemIndex < textDivs.length; itemIndex += 1) {
    const textDiv = textDivs[itemIndex];
    const text = textItems[itemIndex];
    const ranges = rangesByItem[itemIndex];
    if (textDiv === undefined || text === undefined || ranges === undefined || ranges.length === 0)
      continue;
    const fragment = document.createDocumentFragment();
    let offset = 0;
    for (const range of ranges) {
      if (range.start > offset) fragment.append(text.slice(offset, range.start));
      const mark = document.createElement('mark');
      mark.className = 'fm-file-viewer-pdf-highlight';
      if (range.occurrenceIndex === activeOccurrenceIndex) {
        mark.classList.add('fm-file-viewer-pdf-highlight-active');
      }
      mark.textContent = text.slice(range.start, range.end);
      fragment.append(mark);
      offset = range.end;
    }
    if (offset < text.length) fragment.append(text.slice(offset));
    textDiv.replaceChildren(fragment);
  }
}

type PdfRenderTask = ReturnType<PDFPageProxy['render']>;

function fittedPdfScale(
  page: PDFPageProxy,
  containerWidth: number,
  containerHeight: number,
): number {
  const unscaledViewport = page.getViewport({ scale: 1 });
  const widthScale = containerWidth > 0 ? containerWidth / unscaledViewport.width : undefined;
  const heightScale = containerHeight > 0 ? containerHeight / unscaledViewport.height : undefined;
  return (
    Math.min(
      ...[widthScale, heightScale].filter((value): value is number => value !== undefined),
    ) || 1
  );
}

/** Tracks, per canvas, the most recent render call's generation number and in-flight
 * `RenderTask` - pdf.js throws "Cannot use the same canvas during multiple render() operations"
 * if a second `page.render()` starts before the first finishes, which happens in practice: the
 * viewer's `ResizeObserver` can fire (on initial layout) essentially back-to-back with the page's
 * first render call, targeting the same canvas element (it's reused across page navigation, never
 * remounted). Every `renderPdfPageToCanvas` call cancels/supersedes whatever the same canvas was
 * already doing, so only the latest request ever reaches `page.render()`. */
const canvasRenderState = new WeakMap<
  HTMLCanvasElement,
  { generation: number; task?: PdfRenderTask }
>();
const textLayerRenderState = new WeakMap<HTMLElement, { generation: number; task?: TextLayer }>();

function isRenderCancelledException(error: unknown): boolean {
  return (
    typeof error === 'object' &&
    error !== null &&
    'name' in error &&
    (error as { name: unknown }).name === 'RenderingCancelledException'
  );
}

/** Renders one page onto `canvas` at a resolution that fits entirely within
 * `containerWidth`×`containerHeight` (whichever axis is the tighter constraint, so a tall page
 * shrinks to fit vertically and a wide one shrinks to fit horizontally - the same "Fit" semantics
 * as the image viewer's fit-to-container mode). Device-pixel-ratio aware, so text/vector content
 * stays sharp on HiDPI displays. Safe to call again on the same canvas before a prior call has
 * finished - the earlier call is cancelled (see `canvasRenderState`) rather than racing it. */
export async function renderPdfPageToCanvas(
  document: PDFDocumentProxy,
  pageNumber: number,
  canvas: HTMLCanvasElement,
  containerWidth: number,
  containerHeight: number,
  zoom = 1,
): Promise<void> {
  const state = canvasRenderState.get(canvas) ?? { generation: 0 };
  const generation = state.generation + 1;
  state.generation = generation;
  state.task?.cancel();
  canvasRenderState.set(canvas, state);
  const isSuperseded = (): boolean => canvasRenderState.get(canvas)?.generation !== generation;

  const page: PDFPageProxy = await document.getPage(pageNumber);
  if (isSuperseded()) return;
  const scale = fittedPdfScale(page, containerWidth, containerHeight) * zoom;
  const devicePixelRatio = window.devicePixelRatio || 1;
  const viewport = page.getViewport({ scale: scale * devicePixelRatio });
  const stagingCanvas = canvas.ownerDocument.createElement('canvas');
  stagingCanvas.width = Math.ceil(viewport.width);
  stagingCanvas.height = Math.ceil(viewport.height);
  const stagingContext = stagingCanvas.getContext('2d');
  if (stagingContext === null || isSuperseded()) return;

  const task = page.render({ canvas: stagingCanvas, canvasContext: stagingContext, viewport });
  state.task = task;
  try {
    await task.promise;
  } catch (error) {
    if (isRenderCancelledException(error)) return;
    throw error;
  }
  if (isSuperseded()) return;
  canvas.width = stagingCanvas.width;
  canvas.height = stagingCanvas.height;
  canvas.style.width = `${Math.ceil(viewport.width / devicePixelRatio)}px`;
  canvas.style.height = `${Math.ceil(viewport.height / devicePixelRatio)}px`;
  canvas.getContext('2d')?.drawImage(stagingCanvas, 0, 0);
}

/** Renders pdf.js's positioned text layer over a page and highlights every query match. Passing no
 * expression cancels any in-flight text render and clears stale highlights. */
export async function renderPdfSearchHighlights(
  document: PDFDocumentProxy,
  pageNumber: number,
  container: HTMLElement,
  containerWidth: number,
  containerHeight: number,
  expression: RegExp | undefined,
  activeOccurrenceIndex?: number,
  zoom = 1,
): Promise<void> {
  const state = textLayerRenderState.get(container) ?? { generation: 0 };
  const generation = state.generation + 1;
  state.generation = generation;
  state.task?.cancel();
  delete state.task;
  textLayerRenderState.set(container, state);
  container.replaceChildren();
  if (expression === undefined) return;
  const isSuperseded = (): boolean =>
    textLayerRenderState.get(container)?.generation !== generation;

  const page = await document.getPage(pageNumber);
  if (isSuperseded()) return;
  const viewport = page.getViewport({
    scale: fittedPdfScale(page, containerWidth, containerHeight) * zoom,
  });
  container.style.setProperty('--total-scale-factor', String(viewport.scale));
  container.style.width = `${viewport.width}px`;
  container.style.height = `${viewport.height}px`;
  const task = new TextLayer({
    textContentSource: page.streamTextContent(),
    container,
    viewport,
  });
  state.task = task;
  try {
    await task.render();
  } catch (error) {
    if (isSuperseded() || isRenderCancelledException(error)) return;
    throw error;
  }
  if (isSuperseded()) return;
  highlightPdfTextLayer(task.textDivs, task.textContentItemsStr, expression, activeOccurrenceIndex);
}
