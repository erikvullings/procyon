// The "legacy" build (rather than the default `pdfjs-dist` entry) is used deliberately: the
// default build assumes `DOMMatrix`/`Path2D` etc. are always present, which is true in real
// browsers/Tauri's webview but not in the vitest/jsdom test environment - the legacy build detects
// and polyfills for that case, so it works in both without needing a separate test-only mock.
import {
  GlobalWorkerOptions,
  getDocument,
  type PDFDocumentProxy,
  type PDFPageProxy,
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

type PdfRenderTask = ReturnType<PDFPageProxy['render']>;

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
): Promise<void> {
  const state = canvasRenderState.get(canvas) ?? { generation: 0 };
  const generation = state.generation + 1;
  state.generation = generation;
  state.task?.cancel();
  canvasRenderState.set(canvas, state);
  const isSuperseded = (): boolean => canvasRenderState.get(canvas)?.generation !== generation;

  const page: PDFPageProxy = await document.getPage(pageNumber);
  if (isSuperseded()) return;
  const unscaledViewport = page.getViewport({ scale: 1 });
  const widthScale = containerWidth > 0 ? containerWidth / unscaledViewport.width : undefined;
  const heightScale = containerHeight > 0 ? containerHeight / unscaledViewport.height : undefined;
  const scale =
    Math.min(
      ...[widthScale, heightScale].filter((value): value is number => value !== undefined),
    ) || 1;
  const devicePixelRatio = window.devicePixelRatio || 1;
  const viewport = page.getViewport({ scale: scale * devicePixelRatio });
  canvas.width = Math.ceil(viewport.width);
  canvas.height = Math.ceil(viewport.height);
  canvas.style.width = `${Math.ceil(viewport.width / devicePixelRatio)}px`;
  canvas.style.height = `${Math.ceil(viewport.height / devicePixelRatio)}px`;
  const context = canvas.getContext('2d');
  if (context === null || isSuperseded()) return;

  const task = page.render({ canvas, canvasContext: context, viewport });
  state.task = task;
  try {
    await task.promise;
  } catch (error) {
    if (isRenderCancelledException(error)) return;
    throw error;
  }
}
