import m from "mithril";
import { optimizer, vscodeApi } from "./optimizer";
import { Header } from "./components/header";
import { EditorPanel } from "./components/editorPanel";
import { PreviewPanel } from "./components/previewPanel";
import { Sidebar } from "./components/sidebar";
import { canSaveProcyonSvg, handleProcyonMessage, procyonPlugin, saveProcyonSvg } from "./procyon";

let svgScale = 1;
let panX = 0;
let panY = 0;
let isPanning = false;
let startX = 0;
let startY = 0;
let pendingWheelZoom: number | null = null;

const STORAGE_THEME_KEY = "svgo-theme";
const STORAGE_SIDEBAR_KEY = "svgo-sidebar-open";
const STORAGE_SPLITTER_KEY = "svgo-splitter-percent";
const STORAGE_SPLITTER_ORIENTATION_KEY = "svgo-splitter-orientation";
const SPLITTER_MIN_PERCENT = 10;
const SPLITTER_MAX_PERCENT = 90;
const MIN_ZOOM = 0.1;
const MAX_ZOOM = 10;

type SplitOrientation = "vertical" | "horizontal";

function getStoredValue(key: string): string | null {
  try {
    return localStorage.getItem(key);
  } catch {
    return null;
  }
}

function setStoredValue(key: string, value: string): void {
  try {
    localStorage.setItem(key, value);
  } catch {
    // Ignore storage failures (private mode / quota / unavailable storage)
  }
}

function clampSplitterPercent(percent: number): number {
  return Math.max(
    SPLITTER_MIN_PERCENT,
    Math.min(SPLITTER_MAX_PERCENT, percent),
  );
}

function readTheme(): "dark" | "light" | "auto" {
  if (procyonPlugin) {
    return procyonPlugin.theme === "dark" || procyonPlugin.theme === "light"
      ? procyonPlugin.theme
      : "auto";
  }
  const stored = getStoredValue(STORAGE_THEME_KEY);
  if (stored === "dark" || stored === "light" || stored === "auto") {
    return stored;
  }
  return "dark";
}

function readSidebarOpen(): boolean {
  const stored = getStoredValue(STORAGE_SIDEBAR_KEY);
  if (stored === "true") return true;
  if (stored === "false") return false;
  return true;
}

function readSplitterPercent(): number {
  const stored = Number(getStoredValue(STORAGE_SPLITTER_KEY));
  if (!Number.isFinite(stored)) return 50;
  return clampSplitterPercent(stored);
}

function readSplitterOrientation(): SplitOrientation {
  const stored = getStoredValue(STORAGE_SPLITTER_ORIENTATION_KEY);
  if (stored === "horizontal" || stored === "vertical") {
    return stored;
  }
  return "vertical";
}

let theme: "dark" | "light" | "auto" = readTheme();
let sidebarOpen = procyonPlugin ? false : readSidebarOpen();
let splitterPercent = procyonPlugin ? 50 : readSplitterPercent();
let splitterOrientation: SplitOrientation = procyonPlugin ? "vertical" : readSplitterOrientation();
let lastCopiedSvgFingerprint: string | null = null;
let pasteToastMessage = "";
let pasteToastTimer: ReturnType<typeof setTimeout> | null = null;
let saveStatus: { kind: "saving" | "success" | "error"; message: string } | null = null;
let saveStatusTimer: ReturnType<typeof setTimeout> | null = null;
let settingsError: string | null = null;

const showFileActions = !procyonPlugin;
const showDownload = !procyonPlugin;

function fingerprintSvg(svg: string): string {
  const normalized = svg.trim().replace(/\s+/g, " ");
  let hash = 0;
  for (let i = 0; i < normalized.length; i += 1) {
    hash = (hash * 31 + normalized.charCodeAt(i)) >>> 0;
  }
  return `${normalized.length}:${hash}`;
}

function extractValidSvgFromText(text: string): string | null {
  const trimmed = text.trim();
  if (!trimmed || !/<svg\b/i.test(trimmed)) return null;

  const parser = new DOMParser();
  const doc = parser.parseFromString(trimmed, "image/svg+xml");
  const hasParseError = doc.querySelector("parsererror") !== null;
  const rootSvg = doc.querySelector("svg");
  if (hasParseError || !rootSvg) return null;

  return trimmed;
}

function handleGlobalSvgPaste(event: ClipboardEvent): void {
  const target = event.target as HTMLElement | null;
  if (target?.closest && target.closest("#editor")) return;
  if (["INPUT", "TEXTAREA"].includes(target?.tagName || "")) return;

  const clipboardText = event.clipboardData?.getData("text/plain") || "";
  const svgText = extractValidSvgFromText(
    optimizer.fixInvalidHexColors(clipboardText),
  );
  if (!svgText) return;

  const incomingFingerprint = fingerprintSvg(svgText);
  if (
    lastCopiedSvgFingerprint &&
    incomingFingerprint === lastCopiedSvgFingerprint
  ) {
    return;
  }

  event.preventDefault();
  const shouldReplace = window.confirm(
    "Detected SVG content in clipboard. Replace the current SVG document?",
  );
  if (!shouldReplace) return;

  optimizer.loadSvgString(svgText);
  showPasteToast("SVG replaced from clipboard. Press Cmd/Ctrl+Z to undo.");
}

function showPasteToast(message: string): void {
  pasteToastMessage = message;
  if (pasteToastTimer) {
    clearTimeout(pasteToastTimer);
  }
  pasteToastTimer = setTimeout(() => {
    pasteToastMessage = "";
    pasteToastTimer = null;
    m.redraw();
  }, 2800);
  m.redraw();
}

function setSaveStatus(status: typeof saveStatus): void {
  if (saveStatusTimer) clearTimeout(saveStatusTimer);
  saveStatusTimer = null;
  saveStatus = status;
  if (status?.kind === "success") {
    saveStatusTimer = setTimeout(() => {
      saveStatus = null;
      saveStatusTimer = null;
      m.redraw();
    }, 2800);
  }
  m.redraw();
}

function requestProcyonSave(): void {
  if (!canSaveProcyonSvg(optimizer)) return;
  setSaveStatus({ kind: "saving", message: "Saving SVG..." });
  try {
    saveProcyonSvg(optimizer);
  } catch (error) {
    setSaveStatus({
      kind: "error",
      message: `Save failed: ${error instanceof Error ? error.message : String(error)}`,
    });
  }
}

function resolveTheme(nextTheme: "dark" | "light" | "auto") {
  if (nextTheme === "auto") {
    return window.matchMedia &&
      window.matchMedia("(prefers-color-scheme: dark)").matches
      ? "dark"
      : "light";
  }
  return nextTheme;
}

function applyTheme(nextTheme: "dark" | "light" | "auto") {
  theme = nextTheme;
  const resolved = resolveTheme(theme);
  document.body.classList.toggle("theme-light", resolved === "light");
  if (!procyonPlugin) setStoredValue(STORAGE_THEME_KEY, theme);
  optimizer.setEditorTheme(resolved);
  if (procyonPlugin) m.redraw();
}

function toggleTheme() {
  const next = theme === "dark" ? "light" : theme === "light" ? "auto" : "dark";
  applyTheme(next);
}

function toggleSidebar() {
  sidebarOpen = !sidebarOpen;
  if (!procyonPlugin) setStoredValue(STORAGE_SIDEBAR_KEY, String(sidebarOpen));
}

function applySplitterLayout(
  left: HTMLElement,
  right: HTMLElement,
  percent: number,
  totalSize: number,
  orientation: SplitOrientation,
): void {
  const normalizedPercent = clampSplitterPercent(percent);
  const safeTotal = Math.max(1, totalSize);
  const percentSplitter = (6 / safeTotal) * 100;
  const leftPercent = procyonPlugin
    ? normalizedPercent * (1 - percentSplitter / 100)
    : normalizedPercent;
  left.style.flex = `0 0 ${leftPercent}%`;
  right.style.flex = `0 0 ${100 - leftPercent - percentSplitter}%`;

  if (orientation === "horizontal") {
    left.style.minWidth = "0";
    right.style.minWidth = "0";
  }
}

function getSplitterTotalSize(
  container: HTMLElement | null,
  orientation: SplitOrientation,
): number {
  const bounds = container?.getBoundingClientRect();
  if (orientation === "horizontal") {
    return bounds?.width ?? window.innerWidth;
  }
  return bounds?.height ?? window.innerHeight;
}

function setupSplitter(): void {
  const splitter = document.getElementById("dragbar");
  const left = document.getElementById("left-panel");
  const right = document.getElementById("right-panel");
  if (!splitter || !left || !right) return;

  const container = splitter.parentElement as HTMLElement | null;
  const initialTotal = getSplitterTotalSize(container, splitterOrientation);
  applySplitterLayout(
    left,
    right,
    splitterPercent,
    initialTotal,
    splitterOrientation,
  );

  splitter.onmousedown = function (e) {
    e.preventDefault();
    document.onmousemove = function (event) {
      const bounds = container?.getBoundingClientRect();
      const total = getSplitterTotalSize(container, splitterOrientation);
      const offset =
        splitterOrientation === "horizontal"
          ? event.clientX - (bounds?.left ?? 0)
          : event.clientY - (bounds?.top ?? 0);

      splitterPercent = clampSplitterPercent((offset / total) * 100);
      applySplitterLayout(
        left,
        right,
        splitterPercent,
        total,
        splitterOrientation,
      );
    };
    document.onmouseup = function () {
      document.onmousemove = null;
      document.onmouseup = null;
      if (!procyonPlugin) setStoredValue(STORAGE_SPLITTER_KEY, String(splitterPercent));
    };
  };
}

function toggleSplitterOrientation(): void {
  splitterOrientation =
    splitterOrientation === "vertical" ? "horizontal" : "vertical";
  if (!procyonPlugin) setStoredValue(STORAGE_SPLITTER_ORIENTATION_KEY, splitterOrientation);
  setupSplitter();
}

function applyTransform(): void {
  const svg = document.querySelector(
    ".preview-container svg",
  ) as HTMLElement | null;
  if (svg) {
    svg.style.transform = `translate(${panX}px, ${panY}px) scale(${svgScale})`;
    svg.style.transformOrigin = "0 0";
  }
}

function zoomSvg(factor: number): void {
  if (pendingWheelZoom !== null) cancelAnimationFrame(pendingWheelZoom);
  pendingWheelZoom = null;
  svgScale = Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, svgScale * factor));
  applyTransform();
}

function zoomSvgAtCursor(factor: number, clientX: number, clientY: number): void {
  const svg = document.querySelector<SVGSVGElement>(".preview-container svg");
  if (!svg) return;
  const before = svg.getScreenCTM();
  if (!before) return;

  const cursor = svg.createSVGPoint();
  cursor.x = clientX;
  cursor.y = clientY;
  const point = cursor.matrixTransform(before.inverse());
  const previousScale = svgScale;
  zoomSvg(factor);
  if (svgScale === previousScale) return;

  const align = () => {
    const after = svg.getScreenCTM();
    if (!after) return;
    const movedPoint = point.matrixTransform(after);
    panX += clientX - movedPoint.x;
    panY += clientY - movedPoint.y;
    applyTransform();
  };
  align();
  // A scrollbar appearing or disappearing can resize a fitted SVG after this event.
  pendingWheelZoom = requestAnimationFrame(() => {
    pendingWheelZoom = null;
    align();
  });
}

function resetZoom(): void {
  if (pendingWheelZoom !== null) cancelAnimationFrame(pendingWheelZoom);
  pendingWheelZoom = null;
  svgScale = 1;
  panX = 0;
  panY = 0;
  applyTransform();
}

function setupPanEvents(): void {
  const container = document.querySelector(
    ".preview-container",
  ) as HTMLElement | null;
  if (!container) return;

  container.addEventListener("mousedown", (e: MouseEvent) => {
    if (pendingWheelZoom !== null) cancelAnimationFrame(pendingWheelZoom);
    pendingWheelZoom = null;
    isPanning = true;
    startX = e.clientX - panX;
    startY = e.clientY - panY;
    container.style.cursor = "grabbing";
  });

  container.addEventListener("mousemove", (e: MouseEvent) => {
    if (!isPanning) return;
    panX = e.clientX - startX;
    panY = e.clientY - startY;
    applyTransform();
  });

  container.addEventListener("mouseup", () => {
    isPanning = false;
    container.style.cursor = "default";
  });

  container.addEventListener("mouseleave", () => {
    isPanning = false;
    container.style.cursor = "default";
  });

  container.addEventListener(
    "wheel",
    (e: WheelEvent) => {
      e.preventDefault();
      const delta = e.deltaY > 0 ? 0.9 : 1.1;
      zoomSvgAtCursor(delta, e.clientX, e.clientY);
    },
    { passive: false },
  );
}

function copyToClipboard(): void {
  const sourceSvg = optimizer.getSourceSvg();
  if (!sourceSvg) {
    alert("No SVG content to copy");
    return;
  }

  navigator.clipboard
    .writeText(sourceSvg)
    .then(() => {
      lastCopiedSvgFingerprint = fingerprintSvg(sourceSvg);
      optimizer.copyStatus = "copied";
      if (optimizer.copyResetTimer) {
        clearTimeout(optimizer.copyResetTimer);
      }
      optimizer.copyResetTimer = setTimeout(() => {
        optimizer.copyStatus = "idle";
        optimizer.copyResetTimer = null;
        m.redraw();
      }, 2000);
      m.redraw();
    })
    .catch((err) => {
      console.error("Failed to copy:", err);
      alert("Failed to copy to clipboard");
    });
}

export const App: m.Component = {
  oncreate() {
    document.body.classList.toggle("theme-procyon", Boolean(procyonPlugin));
    if (import.meta.env.MODE !== "procyon" && !procyonPlugin) {
      setTimeout(() => {
        optimizer.initializeEditor();
      }, 100);
    }

    applyTheme(theme);

    if (window.matchMedia) {
      const media = window.matchMedia("(prefers-color-scheme: dark)");
      const handleChange = () => {
        if (theme === "auto") applyTheme(theme);
      };
      if (typeof media.addEventListener === "function") {
        media.addEventListener("change", handleChange);
      } else {
        const legacyMedia = media as MediaQueryList & {
          addListener?: (
            listener: (this: MediaQueryList, ev: MediaQueryListEvent) => void,
          ) => void;
        };
        if (typeof legacyMedia.addListener === "function") {
          legacyMedia.addListener(handleChange);
        }
      }
    }

    const dropZone = document.body;
    dropZone.addEventListener("dragover", (e) => {
      e.preventDefault();
      dropZone.classList.add("dragover");
    });

    dropZone.addEventListener("dragleave", () => {
      dropZone.classList.remove("dragover");
    });

    dropZone.addEventListener("drop", (e) => {
      e.preventDefault();
      dropZone.classList.remove("dragover");
      const files = e.dataTransfer?.files;
      if (files && files.length > 0 && files[0].type === "image/svg+xml") {
        optimizer.loadFile(files[0]);
      }
    });
  },

  view() {
    const stats = optimizer.getStats();
    const isCopied = optimizer.copyStatus === "copied";
    const sourceSvg = optimizer.getSourceSvg();
    const previewSvg = optimizer.getPreviewSvg();
    const hasSource = Boolean(sourceSvg && sourceSvg.trim());

    const headerStats = {
      originalSizeLabel: optimizer.formatBytes(stats.originalSize),
      optimizedSizeLabel: optimizer.formatBytes(stats.optimizedSize),
      reductionLabel: `${stats.reduction > 0 ? "-" : ""}${optimizer.formatBytes(Math.abs(stats.reduction))} (${stats.reductionPercent.toFixed(1)}%)`,
      reductionClass:
        stats.reduction > 0
          ? "reduction-positive"
          : stats.reduction < 0
            ? "reduction-negative"
            : "",
    };

    const body: m.Children[] = [
      m(".app-shell", [
        m(Sidebar, {
          optimizer,
          sourceSvg,
          theme,
          onToggleTheme: procyonPlugin ? undefined : toggleTheme,
          open: sidebarOpen,
          showFileActions,
          showDownload,
          onCopy: procyonPlugin ? copyToClipboard : undefined,
          isCopied,
        }),
        m(".app-main", [
          m(Header, {
            stats: headerStats,
            showTitle: !procyonPlugin,
            onToggleSidebar: toggleSidebar,
            canOptimize: hasSource,
            onOptimize: () => optimizer.loadOptimizedFile(),
            canCopy: hasSource,
            isCopied,
            onCopy: copyToClipboard,
            onSave: procyonPlugin ? requestProcyonSave : undefined,
            canSave: procyonPlugin ? canSaveProcyonSvg(optimizer) : false,
          }),
          settingsError
            ? m(".settings-error-banner[role=alert][aria-live=assertive]", settingsError)
            : null,
          m(
            ".main-content",
            {
              class:
                splitterOrientation === "horizontal"
                  ? "is-horizontal"
                  : "is-vertical",
              oncreate: setupSplitter,
              onupdate: setupSplitter,
            },
            [
              m(".editor-panel#left-panel", [m(EditorPanel)]),
              m("div#dragbar.dragbar"),
              m<import("./components/previewPanel").PreviewPanelAttrs, {}>(
                PreviewPanel,
                {
                  previewSvg,
                  theme: resolveTheme(theme),
                  splitOrientation: splitterOrientation,
                  onToggleSplitOrientation: toggleSplitterOrientation,
                  onZoomIn: () => zoomSvg(1.2),
                  onZoomOut: () => zoomSvg(0.8),
                  onResetZoom: () => resetZoom(),
                },
              ),
            ],
          ),
        ]),
      ]),
      pasteToastMessage
        ? m(".app-toast[role=status][aria-live=polite]", pasteToastMessage)
        : null,
      saveStatus
        ? m(
            ".app-toast.save-status",
            {
              class: saveStatus.kind === "error" ? "save-error" : "",
              role: saveStatus.kind === "error" ? "alert" : "status",
              "aria-live": saveStatus.kind === "error" ? "assertive" : "polite",
            },
            saveStatus.message,
          )
        : null,
    ];

    return m("div", body);
  },
};

let globalHandlersInitialized = false;
export function initializeGlobalHandlers() {
  if (globalHandlersInitialized) return;
  globalHandlersInitialized = true;
  setupPanEvents();
  document.addEventListener("paste", handleGlobalSvgPaste);

  if (procyonPlugin) {
    window.addEventListener("message", (event) => {
      const message = handleProcyonMessage(event, optimizer);
      if (!message) return;
      if (message.type === "load-svg") {
        setSaveStatus(null);
      } else if (message.type === "flush-settings") {
        return;
      } else if (message.type === "theme-change") {
        applyTheme(message.theme);
      } else if (message.type === "settings-result") {
        if (message.success) {
          settingsError = null;
        } else {
          settingsError = "error" in message && message.error
            ? `Failed to save editor settings: ${message.error}`
            : "Failed to save editor settings.";
        }
        m.redraw();
      } else if (message.type === "close-result") {
        if (!message.success) setSaveStatus({
          kind: "error",
          message: "error" in message && message.error
            ? `Close failed: ${message.error}` : "Close failed.",
        });
      } else {
        setSaveStatus(message.success
          ? { kind: "success", message: "SVG saved." }
          : {
              kind: "error",
              message: "error" in message && message.error
                ? `Save failed: ${message.error}`
                : "Save failed.",
            });
      }
    });
    window.addEventListener("keydown", (e) => {
      const isMac = navigator.platform.toUpperCase().includes("MAC");
      if (
        (isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey) &&
        !e.altKey &&
        !e.shiftKey &&
        e.key.toLowerCase() === "w"
      ) {
        e.preventDefault();
        e.stopPropagation();
        procyonPlugin.postMessage({ type: "close-panel" });
        return;
      }
      if (
        (e.metaKey || e.ctrlKey) &&
        !e.altKey &&
        !e.shiftKey &&
        e.key.toLowerCase() === "s"
      ) {
        e.preventDefault();
        e.stopPropagation();
        requestProcyonSave();
      }
    }, true);
  } else if (vscodeApi) {
    window.addEventListener("message", (event) => {
      const data = event.data;
      if (!data || typeof data !== "object") return;
      if (data.type === "load-svg") {
        optimizer.loadSvgString(data.svg || "");
      }
    });
  }

  document.addEventListener("keydown", (e) => {
    const target = e.target as HTMLElement | null;
    if (!target) return;
    if (["INPUT", "TEXTAREA"].includes(target.tagName)) return;
    if (target.closest && target.closest(".attr-dialog-backdrop")) return;
    if (target.closest && target.closest("#editor")) return;

    const isMac = navigator.platform.toUpperCase().includes("MAC");
    const modKey = isMac ? e.metaKey : e.ctrlKey;

    if (modKey && !e.shiftKey && e.key.toLowerCase() === "z") {
      e.preventDefault();
      optimizer.undo();
      m.redraw();
      return;
    }

    if (
      modKey &&
      (e.key.toLowerCase() === "y" ||
        (e.shiftKey && e.key.toLowerCase() === "z"))
    ) {
      e.preventDefault();
      optimizer.redo();
      m.redraw();
      return;
    }

    const step = 20;
    const zoomStep = 1.1;
    switch (e.key) {
      case "ArrowUp":
        panY -= step;
        break;
      case "ArrowDown":
        panY += step;
        break;
      case "ArrowLeft":
        panX -= step;
        break;
      case "ArrowRight":
        panX += step;
        break;
      case "+":
      case "=":
        svgScale *= zoomStep;
        break;
      case "-":
      case "_":
        svgScale /= zoomStep;
        break;
      case "0":
        svgScale = 1;
        panX = 0;
        panY = 0;
        break;
      default:
        return;
    }
    applyTransform();
  });
}
