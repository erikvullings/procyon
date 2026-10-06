import m from "mithril";
import { optimize } from "svgo/browser";
import {
  NUMERIC_ATTRS,
  NUMERIC_LIST_ATTRS,
  KNOWN_SVG_ATTRS,
  roundNumericValueFixed,
  roundNumericList,
  roundPathData,
  collapseTransforms,
} from "./svgUtils";
import { sanitizePreviewSvg } from "./svgPreview";
import { procyonPlugin } from "./procyon";

type OptimizeOptions = {
  precision: number;
  pathPrecision: number;
  removeTspan: boolean;
  removeStyling: boolean;
  trimText: boolean;
  autoAutocrop: boolean;
  customWidth: number;
  customHeight: number;
  useCustomDimensions: boolean;
  removeDefaultValues: boolean;
  removeFontFamily: boolean;
  removeFontSize: boolean;
  convertSodipodiArcs: boolean;
  groupSimilarElements: boolean;
  groupTextElementsAtEnd: boolean;
  viewMode: "code" | "tree";
  selectedElementPath: string | null;
  treeDoc: Document | null;
  isUpdatingFromTree: boolean;
};

type HistoryEntry = {
  originalSvg: string;
  optimizedSvg: string;
  options: OptimizeOptions;
};

type PersistedOptions = Pick<
  OptimizeOptions,
  | "precision"
  | "pathPrecision"
  | "removeTspan"
  | "removeStyling"
  | "trimText"
  | "autoAutocrop"
  | "customWidth"
  | "customHeight"
  | "useCustomDimensions"
  | "removeDefaultValues"
  | "removeFontFamily"
  | "removeFontSize"
  | "convertSodipodiArcs"
  | "groupSimilarElements"
  | "groupTextElementsAtEnd"
  | "viewMode"
>;

export type ProcyonOptimizerSettings = Omit<PersistedOptions, "viewMode">;

type PersistedState = {
  sourceSvg: string;
  options: PersistedOptions;
};

export const vscodeApi =
  typeof acquireVsCodeApi === "function" ? acquireVsCodeApi() : undefined;

const PRESERVE_ATTR_NAMES = new Set(["role", "tabindex"]);
const PRESERVE_ATTR_PREFIXES = ["data-", "aria-"];
const BLOCKED_ATTR_PREFIXES = ["inkscape:", "sodipodi:"];
const RESERVED_ATTR_NAME = "data-cx-id";
const PERSISTED_STATE_KEY = "svgo-state-v1";

class SVGOptimizer {
  originalSvg: string;
  optimizedSvg: string;
  editor: MonacoEditorInstance | null;
  editorReady: boolean;
  editorTheme: "dark" | "light";
  history: HistoryEntry[];
  historyPointer: number;
  maxHistory: number;
  options: OptimizeOptions;
  unitConversion: Record<string, number>;
  isRestoringHistory: boolean;
  copyStatus: "idle" | "copied";
  copyResetTimer: ReturnType<typeof setTimeout> | null;
  persistenceEnabled: boolean;
  lastProcyonSettings: string | null;
  procyonSettingsInFlight: { serialized: string; sequence: number } | null;
  queuedProcyonSettings: { serialized: string; settings: ProcyonOptimizerSettings } | null;
  procyonSettingsSequence: number;

  constructor() {
    this.originalSvg = "";
    this.optimizedSvg = "";
    this.editor = null;
    this.editorReady = false;
    this.editorTheme = "dark";
    this.history = []; // Store history for undo/redo
    this.historyPointer = -1; // Pointer to current position in history
    this.maxHistory = 20; // Maximum number of history entries
    this.options = {
      precision: 1,
      pathPrecision: 2,
      removeTspan: true,
      removeStyling: true,
      trimText: true,
      autoAutocrop: false,
      customWidth: 100,
      customHeight: 100,
      useCustomDimensions: false,
      removeDefaultValues: true,
      removeFontFamily: false,
      removeFontSize: false,
      convertSodipodiArcs: true,
      groupSimilarElements: true,
      groupTextElementsAtEnd: false,
      viewMode: procyonPlugin || import.meta.env.MODE === "procyon" ? "tree" : "code",
      selectedElementPath: null, // JSON path or similar to track selected element
      treeDoc: null, // Parsed DOM for the Tree View
      isUpdatingFromTree: false, // Flag to prevent redundant re-parsing
    };
    this.unitConversion = {
      px: 1,
      pt: 1.25, // 1pt = 1.25px (1/72 inch * 96 px/inch)
      pc: 15, // 1pc = 12pt = 15px
      mm: 3.7795275591, // 1mm = 96/25.4 px
      cm: 37.795275591, // 1cm = 10mm
      in: 96, // 1in = 96px
    };
    this.isRestoringHistory = false;
    this.copyStatus = "idle";
    this.copyResetTimer = null;
    this.persistenceEnabled = Boolean(procyonPlugin) || this.canUseLocalStorage();
    this.restoreFromStorage();
    this.lastProcyonSettings = procyonPlugin
      ? JSON.stringify(this.getProcyonSettings())
      : null;
    this.procyonSettingsInFlight = null;
    this.queuedProcyonSettings = null;
    this.procyonSettingsSequence = 0;
    // Initialize with empty state
    this.saveToHistory();

    // Ensure we start with proper history state
    this.historyPointer = 0;
  }

  getSourceSvg(): string {
    if (this.editor) {
      return this.editor.getValue();
    }
    return this.originalSvg;
  }

  canUseLocalStorage(): boolean {
    if (typeof window === "undefined" || typeof localStorage === "undefined") {
      return false;
    }
    if (
      typeof import.meta !== "undefined" &&
      Boolean((import.meta as any).vitest)
    ) {
      return false;
    }
    try {
      const testKey = "__svgo_storage_test__";
      localStorage.setItem(testKey, "1");
      localStorage.removeItem(testKey);
      return true;
    } catch {
      return false;
    }
  }

  getPersistedOptions(): PersistedOptions {
    return {
      precision: this.options.precision,
      pathPrecision: this.options.pathPrecision,
      removeTspan: this.options.removeTspan,
      removeStyling: this.options.removeStyling,
      trimText: this.options.trimText,
      autoAutocrop: this.options.autoAutocrop,
      customWidth: this.options.customWidth,
      customHeight: this.options.customHeight,
      useCustomDimensions: this.options.useCustomDimensions,
      removeDefaultValues: this.options.removeDefaultValues,
      removeFontFamily: this.options.removeFontFamily,
      removeFontSize: this.options.removeFontSize,
      convertSodipodiArcs: this.options.convertSodipodiArcs,
      groupSimilarElements: this.options.groupSimilarElements,
      groupTextElementsAtEnd: this.options.groupTextElementsAtEnd,
      viewMode: this.options.viewMode,
    };
  }

  getProcyonSettings(): ProcyonOptimizerSettings {
    const { viewMode: _viewMode, ...settings } = this.getPersistedOptions();
    return settings;
  }

  applyPersistedOptions(raw: Partial<PersistedOptions>): void {
    const asInt = (
      value: unknown,
      fallback: number,
      min: number,
      max: number,
    ): number => {
      const parsed = Number(value);
      if (!Number.isFinite(parsed)) return fallback;
      const rounded = Math.round(parsed);
      return Math.min(max, Math.max(min, rounded));
    };
    const asBool = (value: unknown, fallback: boolean): boolean =>
      typeof value === "boolean" ? value : fallback;
    const asViewMode = (
      value: unknown,
      fallback: OptimizeOptions["viewMode"],
    ): OptimizeOptions["viewMode"] =>
      value === "code" || value === "tree" ? value : fallback;

    this.options.precision = asInt(raw.precision, this.options.precision, 0, 5);
    this.options.pathPrecision = asInt(
      raw.pathPrecision,
      this.options.pathPrecision,
      0,
      5,
    );
    this.options.customWidth = asInt(
      raw.customWidth,
      this.options.customWidth,
      1,
      100000,
    );
    this.options.customHeight = asInt(
      raw.customHeight,
      this.options.customHeight,
      1,
      100000,
    );
    this.options.removeTspan = asBool(
      raw.removeTspan,
      this.options.removeTspan,
    );
    this.options.removeStyling = asBool(
      raw.removeStyling,
      this.options.removeStyling,
    );
    this.options.trimText = asBool(raw.trimText, this.options.trimText);
    this.options.autoAutocrop = asBool(
      raw.autoAutocrop,
      this.options.autoAutocrop,
    );
    this.options.useCustomDimensions = asBool(
      raw.useCustomDimensions,
      this.options.useCustomDimensions,
    );
    this.options.removeDefaultValues = asBool(
      raw.removeDefaultValues,
      this.options.removeDefaultValues,
    );
    this.options.removeFontFamily = asBool(
      raw.removeFontFamily,
      this.options.removeFontFamily,
    );
    this.options.removeFontSize = asBool(
      raw.removeFontSize,
      this.options.removeFontSize,
    );
    this.options.convertSodipodiArcs = asBool(
      raw.convertSodipodiArcs,
      this.options.convertSodipodiArcs,
    );
    this.options.groupSimilarElements = asBool(
      raw.groupSimilarElements,
      this.options.groupSimilarElements,
    );
    this.options.groupTextElementsAtEnd = asBool(
      raw.groupTextElementsAtEnd,
      this.options.groupTextElementsAtEnd,
    );
    this.options.viewMode = asViewMode(raw.viewMode, this.options.viewMode);
  }

  restoreFromStorage(): void {
    if (!this.persistenceEnabled) return;

    try {
      if (procyonPlugin) {
        const settings: unknown = procyonPlugin.settings;
        if (settings === undefined) return;
        if (!settings || typeof settings !== "object" || Array.isArray(settings)) {
          throw new Error("Invalid host-provided Procyon settings");
        }
        this.applyPersistedOptions(settings as Partial<ProcyonOptimizerSettings>);
        this.options.viewMode = "tree";
        return;
      }

      const raw = localStorage.getItem(PERSISTED_STATE_KEY);
      if (!raw) return;
      const parsed = JSON.parse(raw) as Partial<PersistedState>;

      if (parsed.options && typeof parsed.options === "object") {
        this.applyPersistedOptions(parsed.options);
      }

      if (typeof parsed.sourceSvg === "string" && parsed.sourceSvg.trim()) {
        this.originalSvg = this.normalizeNamespaces(parsed.sourceSvg);
        this.updateTreeDoc();
        void this.optimizeSvg();
      }
    } catch (error) {
      console.warn(
        procyonPlugin
          ? "Failed to restore Procyon settings from host:"
          : "Failed to restore SVGO state from localStorage:",
        error,
      );
    }
  }

  persistState(): void {
    if (!this.persistenceEnabled) return;

    if (procyonPlugin) {
      const settings = this.getProcyonSettings();
      const serialized = JSON.stringify(settings);
      if (this.procyonSettingsInFlight) {
        this.queuedProcyonSettings = serialized === this.procyonSettingsInFlight.serialized
          ? null
          : { serialized, settings };
      } else if (serialized !== this.lastProcyonSettings) {
        this.sendProcyonSettings(settings, serialized);
      }
      return;
    }

    try {
      const state: PersistedState = {
        sourceSvg: this.getSourceSvg() || "",
        options: this.getPersistedOptions(),
      };
      localStorage.setItem(PERSISTED_STATE_KEY, JSON.stringify(state));
    } catch (error) {
      console.warn("Failed to persist SVGO state to localStorage:", error);
    }
  }

  sendProcyonSettings(
    settings: ProcyonOptimizerSettings,
    serialized: string,
    flush = false,
  ): void {
    if (!procyonPlugin) return;
    const sequence = ++this.procyonSettingsSequence;
    this.procyonSettingsInFlight = { serialized, sequence };
    try {
      procyonPlugin.postMessage({
        type: "settings-change", settings, sequence,
        ...(flush ? { flush: true as const } : {}),
      });
    } catch (error) {
      this.procyonSettingsInFlight = null;
      console.warn("Failed to send Procyon settings to host:", error);
    }
  }

  flushProcyonSettings(): void {
    if (!procyonPlugin) return;
    this.queuedProcyonSettings = null;
    const settings = this.getProcyonSettings();
    this.sendProcyonSettings(settings, JSON.stringify(settings), true);
  }

  acknowledgeProcyonSettings(success: boolean, sequence: number): boolean {
    if (!this.procyonSettingsInFlight || sequence !== this.procyonSettingsInFlight.sequence) {
      return false;
    }
    if (success) this.lastProcyonSettings = this.procyonSettingsInFlight.serialized;
    this.procyonSettingsInFlight = null;
    const pending = this.queuedProcyonSettings;
    this.queuedProcyonSettings = null;
    if (pending) this.sendProcyonSettings(pending.settings, pending.serialized);
    return true;
  }

  persistSessionState(): void {
    this.persistState();
  }

  isOptimizationEnabled(): boolean {
    const hasRounding =
      this.options.precision >= 0 || this.options.pathPrecision >= 0;
    const hasToggles =
      this.options.removeDefaultValues ||
      this.options.removeFontFamily ||
      this.options.removeFontSize ||
      this.options.removeTspan ||
      this.options.removeStyling ||
      this.options.trimText ||
      this.options.convertSodipodiArcs ||
      this.options.autoAutocrop ||
      this.options.useCustomDimensions ||
      this.options.groupTextElementsAtEnd;
    const hasGrouping = this.options.groupSimilarElements;
    return hasRounding || hasToggles || hasGrouping;
  }

  getPreviewSvg(): string {
    const sourceSvg = this.getSourceSvg();
    if (!this.isOptimizationEnabled()) {
      return this.normalizeNamespaces(sourceSvg);
    }
    return this.optimizedSvg || this.normalizeNamespaces(sourceSvg);
  }

  async initializeEditor(): Promise<void> {
    if (import.meta.env.MODE === "procyon") return;
    return new Promise<void>((resolve) => {
      require.config({
        paths: {
          vs: "https://cdnjs.cloudflare.com/ajax/libs/monaco-editor/0.52.2/min/vs",
        },
      });
      require(["vs/editor/editor.main"], () => {
        // Wait for the DOM element to be available
        const checkContainer = () => {
          const container = document.getElementById("editor");
          if (container) {
            monaco.languages.register({ id: "xml" });
            monaco.languages.setLanguageConfiguration("xml", {
              brackets: [["<", ">"]],
              autoClosingPairs: [
                { open: "<", close: ">" },
                { open: '"', close: '"' },
                { open: "'", close: "'" },
              ],
            });
            this.editor = monaco.editor.create(container, {
              value:
                this.originalSvg ||
                "<!-- Paste your SVG code here or load a file -->",
              language: "xml",
              theme: this.editorTheme === "dark" ? "vs-dark" : "vs",
              automaticLayout: true,
              minimap: { enabled: false },
              wordWrap: "on",
              renderLineHighlight: "none",
            });

            this.editor.onDidChangeModelContent(() => {
              if (this.options.isUpdatingFromTree || this.isRestoringHistory)
                return;
              const currentValue = this.editor.getValue();
              const normalizedValue = this.normalizeNamespaces(currentValue);
              if (normalizedValue !== currentValue) {
                this.options.isUpdatingFromTree = true;
                this.editor.setValue(normalizedValue);
                this.options.isUpdatingFromTree = false;
              }
              this.originalSvg = normalizedValue;
              this.updateTreeDoc();
              this.optimizeSvg();
              this.saveToHistory(); // Save to history after editing
              m.redraw();
            });

            this.editorReady = true;
            this.applyEditorTheme();
            // Save initial state after editor is initialized
            if (this.history.length === 0) {
              this.saveToHistory();
            }
            m.redraw(); // Trigger a redraw when editor is ready
            resolve();
          } else {
            // If container not found, try again in 50ms
            setTimeout(checkContainer, 50);
          }
        };
        checkContainer();
      });
    });
  }

  applyEditorTheme(): void {
    if (import.meta.env.MODE === "procyon") return;
    if (!this.editor) return;
    if (typeof monaco === "undefined") return;
    const themeName = this.editorTheme === "dark" ? "vs-dark" : "vs";
    monaco.editor.setTheme(themeName);
  }

  setEditorTheme(theme: "dark" | "light"): void {
    this.editorTheme = theme;
    this.applyEditorTheme();
  }

  shouldPreserveAttribute(name: string): boolean {
    if (!name) return false;
    const lower = name.toLowerCase();
    if (lower === RESERVED_ATTR_NAME) return false;
    if (BLOCKED_ATTR_PREFIXES.some((prefix) => lower.startsWith(prefix)))
      return false;
    if (lower.startsWith("xmlns")) return false;
    if (PRESERVE_ATTR_PREFIXES.some((prefix) => lower.startsWith(prefix)))
      return true;
    if (PRESERVE_ATTR_NAMES.has(lower)) return true;
    if (lower.includes(":")) return false;
    return !KNOWN_SVG_ATTRS.has(lower);
  }

  injectPreserveMarkers(svg: string): {
    svg: string;
    preserved: Map<string, Record<string, string>>;
  } {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");
    const preserved = new Map<string, Record<string, string>>();
    let counter = 0;

    doc.querySelectorAll("*").forEach((el) => {
      const attrsToPreserve: Record<string, string> = {};
      Array.from(el.attributes).forEach((attr) => {
        if (this.shouldPreserveAttribute(attr.name)) {
          attrsToPreserve[attr.name] = attr.value;
        }
      });

      if (Object.keys(attrsToPreserve).length > 0) {
        const id = `cx-${counter++}`;
        el.setAttribute(RESERVED_ATTR_NAME, id);
        preserved.set(id, attrsToPreserve);
      }
    });

    return { svg: new XMLSerializer().serializeToString(doc), preserved };
  }

  restorePreservedAttributes(
    svg: string,
    preserved: Map<string, Record<string, string>>,
  ) {
    if (!preserved || preserved.size === 0) return svg;
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");

    preserved.forEach((attrs, id) => {
      const el = doc.querySelector(`[${RESERVED_ATTR_NAME}="${id}"]`);
      if (!el) return;
      Object.entries(attrs).forEach(([name, value]) => {
        if (!el.hasAttribute(name)) {
          el.setAttribute(name, value);
        }
      });
    });

    doc
      .querySelectorAll(`[${RESERVED_ATTR_NAME}]`)
      .forEach((el) => el.removeAttribute(RESERVED_ATTR_NAME));
    return new XMLSerializer().serializeToString(doc);
  }

  loadSvgString(svg: string): void {
    this.originalSvg = this.normalizeNamespaces(svg || "");
    if (this.editor) {
      this.options.isUpdatingFromTree = true;
      this.editor.setValue(this.originalSvg);
      this.options.isUpdatingFromTree = false;
    }
    this.updateTreeDoc();
    this.optimizeSvg();
    this.saveToHistory();
    m.redraw();
  }

  loadFile(file: File): void {
    const reader = new FileReader();
    reader.onload = (e) => {
      this.originalSvg = this.normalizeNamespaces(
        String((e.target && (e.target as FileReader).result) ?? ""),
      );
      if (this.editor) {
        try {
          this.editor.setValue(this.originalSvg);
        } catch (err) {
          console.error("Failed to set editor value:", err);
          // Try to fix common SVG issues before setting
          try {
            // Try to sanitize the SVG by fixing common issues
            this.originalSvg = this.sanitizeSvg(this.originalSvg);
            this.editor.setValue(this.originalSvg);
          } catch (fixErr) {
            console.error("Failed to sanitize SVG:", fixErr);
            // Fall back to showing the SVG in a plain text modal
            alert(
              "The SVG file contains formatting issues that cannot be displayed in the editor. Please try a different file.",
            );
            return;
          }
        }
      } else {
        // If editor isn't ready yet, reinitialize it with the content
        setTimeout(() => {
          if (this.editor) {
            try {
              this.editor.setValue(this.originalSvg);
            } catch (err) {
              console.error("Failed to set editor value (delayed):", err);
            }
          }
        }, 100);
      }
      this.updateTreeDoc();
      this.optimizeSvg();

      // Always save to history after loading
      // First, trim history if we're not at the end
      if (this.historyPointer < this.history.length - 1) {
        // Trim history to current position
        this.history = this.history.slice(0, this.historyPointer + 1);
      }

      // Save current state to history
      this.saveToHistory();

      m.redraw();
    };
    reader.readAsText(file);
  }

  loadOptimizedFile(): void {
    const sourceSvg = this.getSourceSvg();
    if (!sourceSvg || !sourceSvg.trim() || !/<svg\b/i.test(sourceSvg)) {
      this.originalSvg = '<svg xmlns="http://www.w3.org/2000/svg"></svg>';
      if (this.editor) {
        this.options.isUpdatingFromTree = true;
        this.editor.setValue(this.originalSvg);
        this.options.isUpdatingFromTree = false;
      }
      this.updateTreeDoc();
      this.optimizeSvg();
      this.saveToHistory();
      m.redraw();
      return;
    }
    this.optimizeSvg();
    if (this.optimizedSvg) {
      this.originalSvg = this.normalizeNamespaces(this.optimizedSvg);
      if (this.editor) {
        this.options.isUpdatingFromTree = true;
        this.editor.setValue(this.originalSvg);
        this.options.isUpdatingFromTree = false;
      }
      // Update tree doc to reflect the optimized content
      this.updateTreeDoc();
      this.saveToHistory();
      m.redraw();
    }
  }

  autocropCurrentSvg(): void {
    const sourceSvg = this.getSourceSvg();
    if (!sourceSvg || !sourceSvg.trim()) return;
    const croppedSvg = this.autocropSvg(sourceSvg, 3);
    this.originalSvg = croppedSvg;
    if (this.editor) {
      this.options.isUpdatingFromTree = true;
      this.editor.setValue(this.originalSvg);
      this.options.isUpdatingFromTree = false;
    }
    this.updateTreeDoc();
    this.optimizeSvg();
    this.saveToHistory();
    m.redraw();
  }

  removeGroupsCurrentSvg(): void {
    const sourceSvg = this.getSourceSvg();
    if (!sourceSvg || !sourceSvg.trim()) return;
    const ungroupedSvg = this.removeGroups(sourceSvg);
    this.originalSvg = this.normalizeNamespaces(ungroupedSvg);
    if (this.editor) {
      this.options.isUpdatingFromTree = true;
      this.editor.setValue(this.originalSvg);
      this.options.isUpdatingFromTree = false;
    }
    this.updateTreeDoc();
    this.optimizeSvg();
    this.saveToHistory();
    m.redraw();
  }

  roundNumbers(str: string, precision: number): string {
    return this.roundNumbersWithPrecision(str, precision, precision);
  }

  roundNumbersWithPrecision(
    svg: string,
    attrPrecision: number,
    pathPrecision: number,
  ): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");

    doc.querySelectorAll("*").forEach((el) => {
      Array.from(el.attributes).forEach((attr) => {
        const name = attr.name.toLowerCase();
        if (name === "d") {
          attr.value = roundPathData(attr.value, pathPrecision);
          return;
        }
        if (NUMERIC_ATTRS.has(name)) {
          attr.value = roundNumericValueFixed(attr.value, attrPrecision, name);
          return;
        }
        if (NUMERIC_LIST_ATTRS.has(name)) {
          attr.value = roundNumericList(attr.value, attrPrecision, name);
        }
      });
    });

    return new XMLSerializer().serializeToString(doc);
  }

  removeTspanElements(svg: string): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");

    // Remove all tspan elements but keep their text content
    const tspans = doc.querySelectorAll("tspan");
    tspans.forEach((tspan) => {
      const parent = tspan.parentElement;
      if (parent) {
        // Move text content to parent
        if (tspan.textContent) {
          parent.insertBefore(doc.createTextNode(tspan.textContent), tspan);
        }
        parent.removeChild(tspan);
      }
    });

    return new XMLSerializer().serializeToString(doc);
  }

  removeStyling(svg: string) {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");

    // Remove style elements
    const styles = doc.querySelectorAll("style");
    styles.forEach((style) => style.remove());

    // Remove style attributes
    const elements: Element[] = [];
    if (doc.documentElement) elements.push(doc.documentElement);
    doc.querySelectorAll("*").forEach((el) => elements.push(el));

    elements.forEach((el) => {
      el.removeAttribute("style");
      el.removeAttribute("class");
    });

    return new XMLSerializer().serializeToString(doc);
  }

  removeUnusedXlinkNamespace(svg: string): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");
    const root = doc.querySelector("svg");
    if (!root) return svg;

    const XLINK_NS = "http://www.w3.org/1999/xlink";
    const hasXlinkHref = Array.from(doc.querySelectorAll("*")).some(
      (el) =>
        el.getAttributeNS(XLINK_NS, "href") !== null ||
        el.hasAttribute("xlink:href"),
    );
    const hasHref = doc.querySelector("[href]");
    if (!hasXlinkHref && hasHref) {
      root.removeAttribute("xmlns:xlink");
    }
    if (!hasXlinkHref && !hasHref) {
      root.removeAttribute("xmlns:xlink");
    }

    return new XMLSerializer().serializeToString(doc);
  }

  convertXlinkHrefs(svg: string): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");
    const elements = Array.from(doc.querySelectorAll("*"));
    const XLINK_NS = "http://www.w3.org/1999/xlink";
    elements.forEach((el) => {
      const xlinkHref =
        el.getAttributeNS(XLINK_NS, "href") || el.getAttribute("xlink:href");
      if (!xlinkHref) return;
      if (!el.hasAttribute("href")) {
        el.setAttribute("href", xlinkHref);
      }
    });
    return new XMLSerializer().serializeToString(doc);
  }

  removeDanglingNamespacedAttributes(svg: string): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");

    const xmlnsUri = "http://www.w3.org/2000/xmlns/";
    const xmlPrefix = "xml";

    const getDeclaredPrefixes = (start: Element): Set<string> => {
      const declaredPrefixes = new Set<string>([xmlPrefix]);
      let current: Element | null = start;
      while (current) {
        for (const attr of Array.from(current.attributes)) {
          if (attr.namespaceURI === xmlnsUri) {
            if (attr.localName) {
              declaredPrefixes.add(attr.localName);
            }
          }
        }
        current = current.parentElement;
      }
      return declaredPrefixes;
    };

    // Remove elements that still use a prefix with no in-scope declaration.
    Array.from(doc.querySelectorAll("*")).forEach((el) => {
      const prefix = el.prefix || "";
      if (!prefix || prefix === xmlPrefix) return;
      const declaredPrefixes = getDeclaredPrefixes(el);
      if (!declaredPrefixes.has(prefix)) {
        el.remove();
      }
    });

    // Remove attributes that still use a prefix with no in-scope declaration.
    Array.from(doc.querySelectorAll("*")).forEach((el) => {
      const declaredPrefixes = getDeclaredPrefixes(el);
      Array.from(el.attributes).forEach((attr) => {
        if (attr.name === "xmlns" || attr.name.startsWith("xmlns:")) return;
        const prefix = attr.prefix || "";
        if (!prefix || prefix === xmlPrefix) return;
        if (!declaredPrefixes.has(prefix)) {
          el.removeAttribute(attr.name);
        }
      });
    });

    return new XMLSerializer().serializeToString(doc);
  }

  removeSvgRootDefaults(svg: string): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");
    const root = doc.querySelector("svg") || doc.documentElement;
    if (!root) return svg;

    const defaults = {
      version: "1.1",
      baseProfile: "full",
      preserveAspectRatio: "xMidYMid meet",
    };

    Object.entries(defaults).forEach(([attr, value]) => {
      const current = root.getAttribute(attr);
      if (!current) return;
      if (current.trim() === value) {
        root.removeAttribute(attr);
      }
    });

    return new XMLSerializer().serializeToString(doc);
  }

  removeGroups(svg: string): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");

    // Flatten groups by moving their children to parent
    const groups = Array.from(doc.querySelectorAll("g"));
    groups.forEach((group) => {
      const parent = group.parentElement;
      if (parent) {
        // Move all children to parent
        while (group.firstChild) {
          parent.insertBefore(group.firstChild, group);
        }
        parent.removeChild(group);
      }
    });

    return new XMLSerializer().serializeToString(doc);
  }

  /** Tag paths with marker attributes (direct or inherited from an ancestor group)
   *  so SVGO's mergePaths won't combine them. Each path gets a unique counter value
   *  so mergePaths sees differing attributes and refuses to merge them. */
  tagMarkerPaths(svg: string): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");
    const MARKER_ATTRS = ["marker-start", "marker-mid", "marker-end"];

    function hasMarkers(el: Element): boolean {
      return MARKER_ATTRS.some((attr) => el.hasAttribute(attr));
    }

    function ancestorHasMarkers(el: Element): boolean {
      let parent = el.parentElement;
      while (parent) {
        if (hasMarkers(parent)) return true;
        parent = parent.parentElement;
      }
      return false;
    }

    let counter = 0;
    doc.querySelectorAll("path").forEach((path) => {
      if (hasMarkers(path) || ancestorHasMarkers(path)) {
        path.setAttribute("data-no-merge", String(counter++));
      }
    });

    return new XMLSerializer().serializeToString(doc);
  }

  /** Remove the temporary sentinel added by tagMarkerPaths. */
  untagMarkerPaths(svg: string): string {
    return svg.replace(/ data-no-merge="\d+"/g, "");
  }

  mergePathsAndCollapseGroups(svg: string): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");
    const SVG_NS = "http://www.w3.org/2000/svg";

    const mergeableGroupAttrs = new Set([
      "fill",
      "stroke",
      "stroke-width",
      "stroke-linecap",
      "stroke-linejoin",
      "stroke-dasharray",
      "stroke-dashoffset",
      "stroke-miterlimit",
      "fill-rule",
      "opacity",
      "fill-opacity",
      "stroke-opacity",
      "clip-path",
      "mask",
      "filter",
      "vector-effect",
      "paint-order",
      "shape-rendering",
      "text-rendering",
    ]);

    function isPresentationAttr(name: string) {
      return mergeableGroupAttrs.has(name);
    }

    function normalizePathStart(d: string) {
      if (!d) return null;
      const trimmed = d.trim();
      if (!trimmed) return null;
      if (trimmed[0] === "M") return trimmed;
      if (trimmed[0] !== "m") return null;

      const rest = trimmed.slice(1).trim();
      if (!rest) return null;

      const parts = rest.split(/[\s,]+/).filter(Boolean);
      if (parts.length < 2) return null;
      const dx = parts[0];
      const dy = parts[1];
      const tail = parts.slice(2);

      if (tail.length === 0) {
        return `M${dx} ${dy}`;
      }

      const pairs = [];
      for (let i = 0; i < tail.length; i += 2) {
        if (i + 1 >= tail.length) return null;
        pairs.push(`${tail[i]} ${tail[i + 1]}`);
      }

      return `M${dx} ${dy} l ${pairs.join(" l ")}`;
    }

    function hasMarkerAttrs(element: Element) {
      return (
        element.hasAttribute("marker-start") ||
        element.hasAttribute("marker-mid") ||
        element.hasAttribute("marker-end")
      );
    }

    function ancestorHasMarkerAttrs(element: Element) {
      let parent = element.parentElement;
      while (parent) {
        if (hasMarkerAttrs(parent)) return true;
        parent = parent.parentElement;
      }
      return false;
    }

    function canMergeGroup(group: Element) {
      const children = Array.from(group.children);
      if (children.length < 2) return false;
      if (!children.every((child) => child.tagName === "path")) return false;
      if (group.hasAttribute("transform")) return false;
      if (hasMarkerAttrs(group)) return false;
      if (ancestorHasMarkerAttrs(group)) return false;
      if (children.some((child) => hasMarkerAttrs(child))) {
        return false;
      }
      return true;
    }

    function collectGroupAttrs(group: Element) {
      const attrs: Record<string, string> = {};
      Array.from(group.attributes).forEach((attr) => {
        if (isPresentationAttr(attr.name)) {
          attrs[attr.name] = attr.value;
        }
      });
      return attrs;
    }

    function getPathMergeKey(
      path: Element,
      groupAttrs: Record<string, string>,
    ): {
      key: string;
      effectiveAttrs: Record<string, string>;
      normalizedD: string;
    } | null {
      if (path.tagName !== "path") return null;
      if (hasMarkerAttrs(path)) return null;

      const d = path.getAttribute("d") || "";
      const normalizedD = normalizePathStart(d);
      if (!normalizedD) return null;

      const childAttrs: Record<string, string> = {};
      for (const attr of Array.from(path.attributes)) {
        if (attr.name === "d") continue;
        if (!isPresentationAttr(attr.name)) return null;
        childAttrs[attr.name] = attr.value;
      }

      const effectiveAttrs: Record<string, string> = { ...groupAttrs };
      Object.entries(childAttrs).forEach(([name, value]) => {
        effectiveAttrs[name] = value;
      });

      const key = JSON.stringify(
        Object.entries(effectiveAttrs).sort(([a], [b]) => a.localeCompare(b)),
      );

      return { key, effectiveAttrs, normalizedD };
    }

    function createMergedPathForRun(
      run: Element[],
      groupAttrs: Record<string, string>,
      effectiveAttrs: Record<string, string>,
      normalizedDs: string[],
    ): Element | null {
      if (run.length < 2) return null;

      const merged = doc.createElementNS(SVG_NS, "path");
      merged.setAttribute("d", normalizedDs.join(" "));

      Object.entries(effectiveAttrs).forEach(([name, value]) => {
        if (groupAttrs[name] !== value) {
          merged.setAttribute(name, value);
        }
      });

      const estimateOldSize = run.reduce(
        (sum, element) => sum + element.outerHTML.length,
        0,
      );
      const estimateNewSize = merged.outerHTML.length;
      if (estimateNewSize >= estimateOldSize) {
        return null;
      }

      return merged;
    }

    function mergePaths(group: Element) {
      const groupAttrs = collectGroupAttrs(group);
      const children = Array.from(group.children) as Element[];
      let mergedAny = false;
      let i = 0;

      while (i < children.length) {
        const current = children[i];
        const currentInfo = getPathMergeKey(current, groupAttrs);
        if (!currentInfo) {
          i++;
          continue;
        }

        const run = [current];
        const normalizedDs = [currentInfo.normalizedD];
        let j = i + 1;

        while (j < children.length) {
          const next = children[j];
          const nextInfo = getPathMergeKey(next, groupAttrs);
          if (!nextInfo || nextInfo.key !== currentInfo.key) break;
          run.push(next);
          normalizedDs.push(nextInfo.normalizedD);
          j++;
        }

        const merged = createMergedPathForRun(
          run,
          groupAttrs,
          currentInfo.effectiveAttrs,
          normalizedDs,
        );
        if (merged) {
          run[0].parentElement?.insertBefore(merged, run[0]);
          run.forEach((element) => element.remove());
          mergedAny = true;
        }

        i = j;
      }

      return mergedAny;
    }

    const groups = Array.from(doc.querySelectorAll("g"));
    groups.forEach((group) => {
      if (!canMergeGroup(group)) return;
      mergePaths(group);
    });

    // Collapse groups with a single child by moving presentation attrs to child
    const groupsToCollapse = Array.from(doc.querySelectorAll("g"));
    groupsToCollapse.forEach((group) => {
      const hasNonWhitespaceText = Array.from(group.childNodes).some((node) => {
        return node.nodeType === 3 && node.textContent.trim() !== "";
      });
      if (hasNonWhitespaceText) return;

      if (group.hasAttribute("transform")) return;

      const hasNonPresentationAttrs = Array.from(group.attributes).some(
        (attr) => !isPresentationAttr(attr.name),
      );
      if (hasNonPresentationAttrs) return;

      const children = Array.from(group.children);
      if (children.length !== 1) return;
      const child = children[0];

      Array.from(group.attributes).forEach((attr) => {
        if (!isPresentationAttr(attr.name)) return;
        if (!child.hasAttribute(attr.name)) {
          child.setAttribute(attr.name, attr.value);
        }
      });

      group.parentElement.insertBefore(child, group);
      group.remove();
    });

    return new XMLSerializer().serializeToString(doc);
  }

  stripUndeclaredNamespacedContent(svg: string): string {
    const declaredPrefixes = new Set(["xml", "xlink"]);
    const xmlnsRegex =
      /\sxmlns:([A-Za-z_][\w.-]*)\s*=\s*(".*?"|'.*?'|[^\s>]+)/g;
    let match: RegExpExecArray | null;
    while ((match = xmlnsRegex.exec(svg)) !== null) {
      declaredPrefixes.add(match[1]);
    }

    const usedPrefixes = new Set<string>();
    const tagPrefixRegex = /<\s*([A-Za-z_][\w.-]*):[A-Za-z_][\w.-]*/g;
    while ((match = tagPrefixRegex.exec(svg)) !== null) {
      usedPrefixes.add(match[1]);
    }
    const attrPrefixRegex =
      /(\s)([A-Za-z_][\w.-]*):([A-Za-z_][\w.-]*)\s*=\s*(".*?"|'.*?'|[^\s>]+)/g;
    while ((match = attrPrefixRegex.exec(svg)) !== null) {
      usedPrefixes.add(match[2]);
    }

    let output = svg.replace(attrPrefixRegex, (full, space, prefix) => {
      if (prefix === "xmlns") return full;
      if (!declaredPrefixes.has(prefix)) return space;
      return full;
    });

    usedPrefixes.forEach((prefix) => {
      if (prefix === "xml" || prefix === "xlink") return;
      if (declaredPrefixes.has(prefix)) return;
      const selfClosing = new RegExp(`<\\s*${prefix}:[^>]*?/>`, "g");
      const paired = new RegExp(
        `<\\s*${prefix}:[^>]*?>[\\s\\S]*?<\\s*/\\s*${prefix}:[^>]*?>`,
        "g",
      );
      output = output.replace(paired, "");
      output = output.replace(selfClosing, "");
    });

    return output;
  }

  stripXmlDeclarations(svg: string): string {
    return svg
      .replace(/^\uFEFF/, "")
      .replace(/^\s*<\?xml[\s\S]*?\?>\s*/i, "")
      .replace(/^\s*<!doctype[\s\S]*?>\s*/i, "");
  }

  normalizeXlinkHrefs(svg: string): string {
    if (!svg.includes("xlink:href")) return svg;
    let output = svg;
    if (!/\sxmlns:xlink\s*=\s*(".*?"|'.*?')/.test(output)) {
      output = output.replace(
        /<svg\b([^>]*)>/i,
        '<svg$1 xmlns:xlink="http://www.w3.org/1999/xlink">',
      );
    }
    output = output.replace(
      /<[^>]*\bxlink:href\s*=\s*(".*?"|'.*?')[^>]*>/g,
      (tag) => {
        if (/\shref\s*=\s*(".*?"|'.*?')/.test(tag)) return tag;
        return tag.replace(
          /\bxlink:href\s*=\s*(".*?"|'.*?')/,
          (attr, val) => `xlink:href=${val} href=${val}`,
        );
      },
    );
    return output;
  }

  normalizeNamespaces(svg: string): string {
    const cleaned = this.stripXmlDeclarations(svg).replace(/\s+xmlns=(['"])\1/g, "");
    const hexFixed = this.fixInvalidHexColors(cleaned);
    const xlinkNormalized = this.normalizeXlinkHrefs(hexFixed);
    return this.stripUndeclaredNamespacedContent(xlinkNormalized);
  }

  /**
   * Repairs malformed hex colours that Inkscape and some other editors can
   * emit, where a "hex" value contains characters outside the 0-9a-f range
   * (e.g. `fill="#pf5ccc"`). Each out-of-range digit is collapsed to `f`,
   * preserving casing (`G` -> `F`, `g` -> `f`).
   *
   * Only colour-bearing attributes and style/CSS properties are touched, so
   * id references such as `url(#gradient)` or `href="#p"` are left untouched.
   */
  fixInvalidHexColors(svg: string): string {
    if (typeof svg !== "string" || svg.indexOf("#") === -1) return svg;

    // Collapse any non-hex letter (g-z) to f, keeping case.
    const clampHex = (hex: string): string =>
      hex.replace(/[g-z]/gi, (ch) => (ch === ch.toUpperCase() ? "F" : "f"));

    const colorProps =
      "fill|stroke|stop-color|color|flood-color|lighting-color|solid-color|text-decoration-color";

    // Attribute form, e.g. fill="#pf5ccc" or stroke='#0fg'.
    const attrRe = new RegExp(
      `(\\b(?:${colorProps})\\s*=\\s*["'])(#[0-9a-z]{3,8})\\b`,
      "gi",
    );
    // Style / CSS property form, e.g. style="fill:#pf5ccc" or in <style> blocks.
    const styleRe = new RegExp(
      `(\\b(?:${colorProps})\\s*:\\s*)(#[0-9a-z]{3,8})\\b`,
      "gi",
    );

    return svg
      .replace(attrRe, (_m, pre, hex) => pre + clampHex(hex))
      .replace(styleRe, (_m, pre, hex) => pre + clampHex(hex));
  }

  removeDefaultValues(svg: string): string {
    const parser = new DOMParser();
    let doc = parser.parseFromString(svg, "image/svg+xml");
    if (doc.getElementsByTagName("parsererror").length > 0) {
      const cleaned = this.normalizeNamespaces(svg);
      if (cleaned !== svg) {
        const retry = parser.parseFromString(cleaned, "image/svg+xml");
        if (retry.getElementsByTagName("parsererror").length === 0) {
          svg = cleaned;
          doc = retry;
        } else {
          return svg;
        }
      } else {
        return svg;
      }
    }

    const hasAncestorAttrOrStyle = (el: Element, attr: string): boolean => {
      const attrLower = attr.toLowerCase();
      let current = el.parentElement;
      while (current) {
        if (current.hasAttribute(attr)) return true;
        const style = current.getAttribute("style");
        if (style) {
          const matches = style
            .toLowerCase()
            .split(";")
            .some((entry) => entry.trim().startsWith(`${attrLower}:`));
          if (matches) return true;
        }
        current = current.parentElement;
      }
      return false;
    };

    const isZeroLikeValue = (value: string | null): boolean => {
      if (!value) return false;
      return /^[-+]?0*\.?0+(?:e[-+]?\d+)?(?:[a-z%]+)?$/i.test(value.trim());
    };

    // xml:space="preserve" only matters when it actually protects meaningful
    // whitespace. Text with multiple tspans is left untouched entirely (too
    // fragile to reason about which whitespace matters).
    const elementRequiresXmlSpacePreserve = (el: Element): boolean => {
      const tagName = (el.localName || el.tagName).toLowerCase();
      if (tagName === "text" || tagName === "tspan") {
        if (tagName === "text" && el.querySelectorAll("tspan").length > 1) return true;
        return /\S\s+\S/.test(el.textContent ?? "");
      }
      return Array.from(el.querySelectorAll("text")).some((textEl) =>
        elementRequiresXmlSpacePreserve(textEl),
      );
    };

    const defaultValues = {
      "letter-spacing": ["0", "normal"],
      "word-spacing": ["0", "normal"],
      "paint-order": ["normal", "fill stroke markers", "markers stroke fill"],
      "fill-opacity": ["1"],
      "stroke-opacity": ["1"],
      "stroke-width": ["1"],
      "stroke-dasharray": ["none"],
      "stroke-dashoffset": ["0"],
      overflow: ["visible"],
      opacity: ["1"],
      "clip-rule": ["nonzero"],
      "fill-rule": ["nonzero"],
      "stroke-miterlimit": ["4"],
      "stroke-linecap": ["butt"],
      "stroke-linejoin": ["miter", "round"],
      "xml:space": ["preserve"],
      "font-weight": ["400"],
    };
    const inheritedDefaults = new Set([
      "stroke-width",
      "stroke-dasharray",
      "stroke-dashoffset",
    ]);
    const stripAttrNames = new Set(["enable-background"]);
    const stripNamespacedAttrs = new Set([
      "viewOrigin",
      "rulerOrigin",
      "pageBounds",
    ]);

    const elements: Element[] = [];
    if (doc.documentElement) elements.push(doc.documentElement);
    doc.querySelectorAll("*").forEach((el) => elements.push(el));

    elements.forEach((el) => {
      Array.from(el.attributes).forEach((attr) => {
        const localName = attr.localName || attr.name;
        if (stripAttrNames.has(attr.name) || stripAttrNames.has(localName)) {
          el.removeAttribute(attr.name);
          return;
        }
        if (stripNamespacedAttrs.has(localName)) {
          el.removeAttribute(attr.name);
        }
      });

      Object.keys(defaultValues).forEach((attr) => {
        if (
          attr === "overflow" &&
          (el.localName || el.tagName).toLowerCase() === "marker"
        ) {
          return;
        }
        let value = el.getAttribute(attr);
        if (!value && attr === "xml:space") {
          value = el.getAttributeNS(
            "http://www.w3.org/XML/1998/namespace",
            "space",
          );
        }
        const normalized = value ? value.trim() : value;
        const normalizedLower = normalized
          ? normalized.toLowerCase()
          : normalized;
        const isDefault =
          normalized &&
          defaultValues[attr].some(
            (v) => v === normalized || v.toLowerCase() === normalizedLower,
          );
        if (isDefault) {
          if (inheritedDefaults.has(attr) && hasAncestorAttrOrStyle(el, attr)) {
            return;
          }
          if (attr === "xml:space") {
            if (elementRequiresXmlSpacePreserve(el)) return;
            el.removeAttribute("xml:space");
            el.removeAttributeNS(
              "http://www.w3.org/XML/1998/namespace",
              "space",
            );
          } else {
            el.removeAttribute(attr);
          }
        }
      });

      if ((el.localName || el.tagName).toLowerCase() === "marker") {
        ["refX", "refY"].forEach((attr) => {
          if (isZeroLikeValue(el.getAttribute(attr))) {
            el.removeAttribute(attr);
          }
        });
      }

      // Special handling for opacity >= 0.9
      const opacityValue = el.getAttribute("opacity");
      if (opacityValue) {
        const opacity = parseFloat(opacityValue);
        if (opacity >= 0.9) {
          el.removeAttribute("opacity");
        }
      }
    });

    this.hoistXmlSpacePreserve(doc);

    const usedPrefixes = new Set<string>();
    elements.forEach((el) => {
      if (el.prefix) {
        usedPrefixes.add(el.prefix);
      }
      Array.from(el.attributes).forEach((attr) => {
        if (attr.prefix) {
          usedPrefixes.add(attr.prefix);
        }
      });
    });

    elements.forEach((el) => {
      Array.from(el.attributes).forEach((attr) => {
        if (!attr.name.startsWith("xmlns:")) return;
        const prefix = attr.name.slice("xmlns:".length);
        if (prefix === "xml" || prefix === "xlink") return;
        if (!usedPrefixes.has(prefix)) {
          el.removeAttribute(attr.name);
        }
      });
    });

    const declaredPrefixes = new Set<string>(["xml", "xlink"]);
    elements.forEach((el) => {
      Array.from(el.attributes).forEach((attr) => {
        if (attr.name.startsWith("xmlns:")) {
          declaredPrefixes.add(attr.name.slice("xmlns:".length));
        }
      });
    });

    elements.forEach((el) => {
      if (el.prefix && !declaredPrefixes.has(el.prefix)) {
        el.remove();
        return;
      }
      Array.from(el.attributes).forEach((attr) => {
        const prefix = attr.prefix;
        if (!prefix) return;
        if (prefix === "xml" || prefix === "xlink") return;
        if (!declaredPrefixes.has(prefix)) {
          el.removeAttribute(attr.name);
        }
      });
    });

    return new XMLSerializer().serializeToString(doc);
  }

  trimTextContent(svg: string): string {
    const doc = new DOMParser().parseFromString(svg, "image/svg+xml");
    if (doc.querySelector("parsererror")) return svg;

    const XML_NS = "http://www.w3.org/XML/1998/namespace";
    const getXmlSpace = (el: Element): string | null => {
      for (let current: Element | null = el; current; current = current.parentElement) {
        const value =
          current.getAttribute("xml:space") ?? current.getAttributeNS(XML_NS, "space");
        if (value !== null) return value;
      }
      return null;
    };

    doc.querySelectorAll("text").forEach((textEl) => {
      // Multiple tspans are too fragile to reason about — leave untouched.
      if (textEl.querySelectorAll("tspan").length > 1) return;
      // xml:space="preserve" means whitespace is significant — don't trim.
      if (getXmlSpace(textEl) === "preserve") return;

      const trimChildText = (el: Element): void => {
        if (getXmlSpace(el) === "preserve") return;
        Array.from(el.childNodes).forEach((node) => {
          if (node.nodeType === 3) {
            node.textContent = (node.textContent ?? "").trim();
          }
        });
      };
      trimChildText(textEl);
      const tspan = textEl.querySelector("tspan");
      if (tspan) trimChildText(tspan);
    });
    return new XMLSerializer().serializeToString(doc);
  }

  removeFontAttributes(
    svg: string,
    removeFontFamily: boolean,
    removeFontSize: boolean,
  ): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");

    const allElements = doc.querySelectorAll("*");
    allElements.forEach((el) => {
      if (removeFontFamily) {
        el.removeAttribute("font-family");
      }
      if (removeFontSize) {
        el.removeAttribute("font-size");
      }
    });

    return new XMLSerializer().serializeToString(doc);
  }

  moveTextElementsToEnd(svg: string): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");
    const root = doc.querySelector("svg");
    if (!root) return svg;

    // Collect all <text> elements in document order and move them to a trailing root group.
    const textElements = Array.from(doc.querySelectorAll("text"));
    if (textElements.length < 2) return svg;

    const inheritableTextAttrs = [
      "font-family",
      "font-size",
      "font-weight",
      "font-style",
      "font-variant",
      "text-anchor",
      "dominant-baseline",
      "letter-spacing",
      "word-spacing",
      "fill",
      "fill-opacity",
      "stroke",
      "stroke-width",
      "stroke-opacity",
      "opacity",
    ];

    const getStyleProp = (style: string | null, prop: string): string | null => {
      if (!style) return null;
      const escaped = prop.replace(/[-/\\^$*+?.()|[\]{}]/g, "\\$&");
      const match = style.match(new RegExp(`(?:^|;)\\s*${escaped}\\s*:\\s*([^;]+)`, "i"));
      return match ? match[1].trim() : null;
    };

    const resolveInheritedAttr = (el: Element, attr: string): string | null => {
      let current = el.parentElement;
      while (current) {
        const attrValue = current.getAttribute(attr);
        if (attrValue !== null) return attrValue;
        const styled = getStyleProp(current.getAttribute("style"), attr);
        if (styled !== null) return styled;
        current = current.parentElement;
      }
      return null;
    };

    const textGroup = doc.createElementNS("http://www.w3.org/2000/svg", "g");
    const touchedGroups = new Set<Element>();

    const collectAncestorGroups = (el: Element): void => {
      let current = el.parentElement;
      while (current) {
        if (current.tagName.toLowerCase() === "g") touchedGroups.add(current);
        current = current.parentElement;
      }
    };

    const groupHasRenderableContent = (group: Element): boolean => {
      return Array.from(group.childNodes).some((node) => {
        if (node.nodeType === 1) return true;
        if (node.nodeType === 3) return (node.textContent || "").trim() !== "";
        return false;
      });
    };

    textElements.forEach((textEl) => {
      collectAncestorGroups(textEl);
      inheritableTextAttrs.forEach((attr) => {
        if (textEl.hasAttribute(attr)) return;
        const inherited = resolveInheritedAttr(textEl, attr);
        if (inherited !== null) {
          textEl.setAttribute(attr, inherited);
        }
      });
      textGroup.appendChild(textEl);
    });

    // Hoist common inheritable attributes to the group and strip duplicates on children.
    inheritableTextAttrs.forEach((attr) => {
      const firstValue = textElements[0].getAttribute(attr);
      if (firstValue === null) return;
      const allMatch = textElements.every(
        (textEl) => textEl.getAttribute(attr) === firstValue,
      );
      if (!allMatch) return;
      textGroup.setAttribute(attr, firstValue);
      textElements.forEach((textEl) => textEl.removeAttribute(attr));
    });

    // Remove groups that only became empty because text nodes were moved out.
    const getDepth = (el: Element): number => {
      let depth = 0;
      let current: Element | null = el;
      while (current) {
        depth += 1;
        current = current.parentElement;
      }
      return depth;
    };

    Array.from(touchedGroups)
      .sort((a, b) => getDepth(b) - getDepth(a))
      .forEach((group) => {
        if (groupHasRenderableContent(group)) return;
        group.remove();
      });

    root.appendChild(textGroup);

    return new XMLSerializer().serializeToString(doc);
  }

  getGroupableAttributes(
    el: Element,
    groupableAttributes: string[],
  ): Record<string, string> {
    const attributes: Record<string, string> = {};
    groupableAttributes.forEach((attr) => {
      const value = el.getAttribute(attr);
      if (value !== null) {
        attributes[attr] = value;
      }
    });
    return attributes;
  }

  intersectGroupableAttributes(
    base: Record<string, string>,
    next: Record<string, string>,
  ): Record<string, string> {
    const intersection: Record<string, string> = {};
    Object.keys(base).forEach((attr) => {
      if (next[attr] === base[attr]) {
        intersection[attr] = base[attr];
      }
    });
    return intersection;
  }

  estimateGroupSavings(
    commonAttributes: Record<string, string>,
    elementCount: number,
  ): number {
    if (elementCount < 2) return 0;
    const attrSize = Object.entries(commonAttributes).reduce(
      (sum, [attr, value]) => {
        return sum + ` ${attr}="${value}"`.length;
      },
      0,
    );
    const groupOverhead = 7; // "<g></g>"
    return (elementCount - 1) * attrSize - groupOverhead;
  }

  groupRunByAttributes(
    parent: Element,
    run: Element[],
    groupableAttributes: string[],
  ): void {
    const grouped = new Set<Element>();
    let i = 0;

    while (i < run.length) {
      if (grouped.has(run[i])) {
        i++;
        continue;
      }

      let commonAttributes = this.getGroupableAttributes(
        run[i],
        groupableAttributes,
      );
      if (Object.keys(commonAttributes).length === 0) {
        i++;
        continue;
      }

      let bestGroup: {
        end: number;
        attrs: Record<string, string>;
        savings: number;
      } | null = null;
      let currentCommon: Record<string, string> = { ...commonAttributes };

      for (let j = i + 1; j < run.length; j++) {
        if (grouped.has(run[j])) break;

        const nextAttributes = this.getGroupableAttributes(
          run[j],
          groupableAttributes,
        );
        currentCommon = this.intersectGroupableAttributes(
          currentCommon,
          nextAttributes,
        );
        if (Object.keys(currentCommon).length === 0) break;

        const count = j - i + 1;
        const savings = this.estimateGroupSavings(currentCommon, count);
        if (savings > 0 && (!bestGroup || savings > bestGroup.savings)) {
          bestGroup = { end: j, attrs: { ...currentCommon }, savings };
        }
      }

      if (bestGroup) {
        const SVG_NS = "http://www.w3.org/2000/svg";
        const elementsToGroup = run.slice(i, bestGroup.end + 1);
        const group = parent.ownerDocument.createElementNS(SVG_NS, "g");

        Object.entries(bestGroup.attrs).forEach(([attr, value]) => {
          group.setAttribute(attr, value);
        });

        parent.insertBefore(group, elementsToGroup[0]);

        elementsToGroup.forEach((el) => {
          Object.keys(bestGroup.attrs).forEach((attr) => {
            el.removeAttribute(attr);
          });
          group.appendChild(el);
          grouped.add(el);
        });

        i = bestGroup.end + 1;
      } else {
        i++;
      }
    }
  }

  groupElementsByCommonAttributes(
    doc: Document,
    tagNames: string[],
    groupableAttributes: string[],
  ): void {
    const tagSet = new Set(tagNames.map((name) => name.toLowerCase()));
    const parents = new Set<Element>();

    doc.querySelectorAll(tagNames.join(",")).forEach((el) => {
      if (el.parentElement) parents.add(el.parentElement);
    });

    parents.forEach((parent) => {
      const children = Array.from(parent.children) as Element[];
      let i = 0;

      while (i < children.length) {
        const child = children[i];
        const childTag = child.tagName.toLowerCase();
        if (!tagSet.has(childTag)) {
          i++;
          continue;
        }

        const tagName = childTag;
        let runEnd = i;
        while (
          runEnd + 1 < children.length &&
          children[runEnd + 1].tagName.toLowerCase() === tagName
        ) {
          runEnd++;
        }

        const run = children.slice(i, runEnd + 1);
        this.groupRunByAttributes(parent, run, groupableAttributes);
        i = runEnd + 1;
      }
    });
  }

  /**
   * Groups <text> siblings that share the same typography/presentation
   * attributes, even when they aren't adjacent. Unlike shapes, text
   * elements don't stack visually, so reordering them to sit together
   * doesn't change how the SVG renders.
   */
  groupTextElementsByAttributes(
    doc: Document,
    groupableAttributes: string[],
  ): void {
    const parents = new Set<Element>();
    doc.querySelectorAll("text").forEach((el) => {
      if (el.parentElement) parents.add(el.parentElement);
    });

    parents.forEach((parent) => {
      const textChildren = Array.from(parent.children).filter(
        (el) => el.tagName.toLowerCase() === "text",
      ) as Element[];
      if (textChildren.length < 2) return;

      const grouped = new Set<Element>();

      textChildren.forEach((anchor, i) => {
        if (grouped.has(anchor)) return;

        const anchorAttrs = this.getGroupableAttributes(
          anchor,
          groupableAttributes,
        );
        if (Object.keys(anchorAttrs).length === 0) return;

        // Greedily accumulate the largest common attribute subset across any
        // (not necessarily adjacent) remaining text siblings — text doesn't
        // stack visually, so partial matches (e.g. shared font-size/fill but
        // differing stroke) can still be grouped on their shared attributes.
        let currentCommon = { ...anchorAttrs };
        const candidates: Element[] = [anchor];
        let bestGroup: {
          elements: Element[];
          attrs: Record<string, string>;
          savings: number;
        } | null = null;

        for (let j = i + 1; j < textChildren.length; j++) {
          const candidate = textChildren[j];
          if (grouped.has(candidate)) continue;

          const candidateAttrs = this.getGroupableAttributes(
            candidate,
            groupableAttributes,
          );
          const nextCommon = this.intersectGroupableAttributes(
            currentCommon,
            candidateAttrs,
          );
          if (Object.keys(nextCommon).length === 0) continue;

          currentCommon = nextCommon;
          candidates.push(candidate);

          const savings = this.estimateGroupSavings(
            currentCommon,
            candidates.length,
          );
          if (savings > 0 && (!bestGroup || savings > bestGroup.savings)) {
            bestGroup = {
              elements: [...candidates],
              attrs: { ...currentCommon },
              savings,
            };
          }
        }

        if (!bestGroup) return;

        const SVG_NS = "http://www.w3.org/2000/svg";
        const group = parent.ownerDocument.createElementNS(SVG_NS, "g");
        Object.entries(bestGroup.attrs).forEach(([attr, value]) => {
          group.setAttribute(attr, value);
        });

        parent.insertBefore(group, bestGroup.elements[0]);
        bestGroup.elements.forEach((el) => {
          Object.keys(bestGroup.attrs).forEach((attr) =>
            el.removeAttribute(attr),
          );
          group.appendChild(el);
          grouped.add(el);
        });
      });
    });
  }

  groupSimilarElementsByType(svg: string): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");

    // Process different element types separately
    const elementTypes = [
      "path",
      "circle",
      "ellipse",
      "rect",
      "line",
      "polyline",
      "polygon",
    ];

    // Attributes that make sense to group for these element types
    const groupableAttributes = [
      "fill",
      "stroke",
      "stroke-width",
      "stroke-linecap",
      "stroke-linejoin",
      "stroke-dasharray",
      "stroke-dashoffset",
      "stroke-miterlimit",
      "fill-rule",
      "opacity",
      "fill-opacity",
      "stroke-opacity",
    ];

    this.groupElementsByCommonAttributes(
      doc,
      elementTypes,
      groupableAttributes,
    );

    // Text elements share a different set of meaningful presentation
    // attributes (typography), so group them separately from shapes.
    const textGroupableAttributes = [
      "fill",
      "font-family",
      "font-size",
      "font-weight",
      "font-style",
      "font-variant",
      "text-anchor",
      "dominant-baseline",
      "letter-spacing",
      "word-spacing",
      "opacity",
      "fill-opacity",
      "stroke",
      "stroke-width",
      "stroke-opacity",
    ];

    this.groupTextElementsByAttributes(doc, textGroupableAttributes);
    this.hoistXmlSpacePreserve(doc);

    return new XMLSerializer().serializeToString(doc);
  }

  /**
   * Moves `xml:space="preserve"` from text children onto their group when every
   * element child of that group carries it: xml:space is inherited, so the
   * rendering is unchanged and the markup is not repeated per text.
   */
  hoistXmlSpacePreserve(doc: Document): void {
    const XML_NS = "http://www.w3.org/XML/1998/namespace";
    const xmlSpace = (el: Element) =>
      el.getAttributeNS(XML_NS, "space") ?? el.getAttribute("xml:space");
    const groups = Array.from(doc.querySelectorAll("g")).reverse();
    groups.forEach((group) => {
      if (xmlSpace(group) !== null) return;
      const children = Array.from(group.children);
      if (
        children.length < 2 ||
        !children.some((child) => child.localName === "text") ||
        !children.every((child) => xmlSpace(child)?.trim() === "preserve")
      ) {
        return;
      }
      children.forEach((child) => {
        child.removeAttributeNS(XML_NS, "space");
        child.removeAttribute("xml:space");
      });
      group.setAttributeNS(XML_NS, "xml:space", "preserve");
    });
  }

  combinePaths(svg: string): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");
    const SVG_NS = "http://www.w3.org/2000/svg";

    const pathElements = Array.from(doc.querySelectorAll("path")) as Element[];
    if (pathElements.length < 2) return svg;

    // Group paths by their attributes (excluding 'd')
    const pathGroups = new Map<
      string,
      { attributes: Record<string, string>; paths: Element[] }
    >();

    pathElements.forEach((path) => {
      const attributes: Record<string, string> = {};
      Array.from(path.attributes).forEach((attr) => {
        if (attr.name !== "d") {
          attributes[attr.name] = attr.value;
        }
      });

      const signature = JSON.stringify(attributes);
      if (!pathGroups.has(signature)) {
        pathGroups.set(signature, { attributes, paths: [] });
      }
      const entry = pathGroups.get(signature);
      if (entry) entry.paths.push(path);
    });

    // Combine paths with identical attributes
    pathGroups.forEach(({ attributes, paths }) => {
      if (paths.length >= 2) {
        // Combine d attributes
        const combinedD = paths.map((path) => path.getAttribute("d")).join(" ");

        // Create new combined path
        const combinedPath = doc.createElementNS(SVG_NS, "path");
        combinedPath.setAttribute("d", combinedD);

        // Add other attributes
        Object.entries(attributes).forEach(([attr, value]) => {
          combinedPath.setAttribute(attr, value);
        });

        // Replace first path with combined path
        const firstPath = paths[0];
        firstPath.parentNode.insertBefore(combinedPath, firstPath);

        // Remove original paths
        paths.forEach((path) => path.remove());
      }
    });

    return new XMLSerializer().serializeToString(doc);
  }

  removeStrokeFromText(svg: string): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");

    // Find all text elements and remove stroke-related attributes
    const textElements = doc.querySelectorAll("text, tspan");
    textElements.forEach((el) => {
      el.removeAttribute("stroke");
      el.removeAttribute("stroke-width");
      el.removeAttribute("stroke-opacity");
      el.removeAttribute("stroke-dasharray");
      el.removeAttribute("stroke-dashoffset");
      el.removeAttribute("stroke-linecap");
      el.removeAttribute("stroke-linejoin");
      el.removeAttribute("stroke-miterlimit");
    });

    return new XMLSerializer().serializeToString(doc);
  }

  removeDuplicateDefs(svg: string): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");

    const defsElement = doc.querySelector("defs");
    if (!defsElement) return svg;

    // Get all children of defs
    const defChildren = Array.from(defsElement.children);
    const defsByContent = new Map<string, string>();
    const idMappings = new Map<string, string>();

    defChildren.forEach((child) => {
      // Create a normalized string representation of the element (without id)
      const childClone = child.cloneNode(true) as Element;
      childClone.removeAttribute("id");
      const normalized = new XMLSerializer().serializeToString(childClone);

      const originalId = child.getAttribute("id") || "";

      if (defsByContent.has(normalized)) {
        // This is a duplicate - map its ID to the first occurrence's ID
        const firstId = defsByContent.get(normalized);
        idMappings.set(originalId, firstId);

        // Remove the duplicate element
        child.remove();
      } else {
        // This is the first occurrence
        defsByContent.set(normalized, originalId);
      }
    });

    // Update all references in the document
    if (idMappings.size > 0) {
      const svgString = new XMLSerializer().serializeToString(doc);
      let updatedSvg = svgString;

      idMappings.forEach((newId, oldId) => {
        // Replace url(#oldId) with url(#newId)
        updatedSvg = updatedSvg.replace(
          new RegExp(`url\\(#${oldId}\\)`, "g"),
          `url(#${newId})`,
        );
        // Replace #oldId with #newId in other contexts
        updatedSvg = updatedSvg.replace(
          new RegExp(`#${oldId}`, "g"),
          `#${newId}`,
        );
      });

      return updatedSvg;
    }

    return new XMLSerializer().serializeToString(doc);
  }

  convertSodipodiArcs(svg: string): string {
    const SODIPODI_NS = "http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd";
    const SVG_NS = "http://www.w3.org/2000/svg";

    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");

    const sodipodiArcs = Array.from(
      doc.querySelectorAll("circle, ellipse"),
    ).filter((el) => el.getAttributeNS(SODIPODI_NS, "type") === "arc");

    sodipodiArcs.forEach((arc) => {
      try {
        const cx = parseFloat(arc.getAttributeNS(SODIPODI_NS, "cx") || "0");
        const cy = parseFloat(arc.getAttributeNS(SODIPODI_NS, "cy") || "0");
        let rx = parseFloat(arc.getAttributeNS(SODIPODI_NS, "rx") || "0");
        let ry = parseFloat(arc.getAttributeNS(SODIPODI_NS, "ry") || "0");
        let startAngle = parseFloat(
          arc.getAttributeNS(SODIPODI_NS, "start") || "0",
        );
        let endAngle = parseFloat(
          arc.getAttributeNS(SODIPODI_NS, "end") || `${2 * Math.PI}`,
        );

        // Ensure radii are positive
        rx = Math.abs(rx);
        ry = Math.abs(ry);

        if (rx <= 0 || ry <= 0) {
          console.warn(
            "Skipping sodipodi arc with zero or negative radius:",
            arc,
          );
          // Clean up sodipodi attributes even if not converted
          Array.from(arc.attributes).forEach((attr) => {
            if (attr.name.startsWith("sodipodi:")) {
              arc.removeAttribute(attr.name);
            }
          });
          return; // Skip this arc
        }

        // Normalize angles to be within [0, 2*PI)
        startAngle = startAngle % (2 * Math.PI);
        if (startAngle < 0) startAngle += 2 * Math.PI;
        endAngle = endAngle % (2 * Math.PI);
        if (endAngle < 0) endAngle += 2 * Math.PI;

        const epsilon = 1e-6; // Small value for float comparison

        // Check if it's a full circle (or very close to it) AND a perfect circle (rx == ry)
        const isFullCircle =
          Math.abs(endAngle - startAngle) < epsilon ||
          Math.abs(Math.abs(endAngle - startAngle) - 2 * Math.PI) < epsilon;
        const isPerfectCircle = Math.abs(rx - ry) < epsilon;

        let newElement;

        if (isFullCircle && isPerfectCircle) {
          // Convert to a <circle> element for full circles
          newElement = doc.createElementNS(SVG_NS, "circle");
          newElement.setAttribute("cx", cx.toString());
          newElement.setAttribute("cy", cy.toString());
          newElement.setAttribute("r", rx.toString()); // For a circle, rx is the radius
        } else {
          // Convert to a <path> element for arcs/ellipses
          newElement = doc.createElementNS(SVG_NS, "path");
          const pathData = this.createEllipticalArcPath(
            cx,
            cy,
            rx,
            ry,
            startAngle,
            endAngle,
          );
          newElement.setAttribute("d", pathData);
        }

        // Copy all non-sodipodi attributes to the new element
        Array.from(arc.attributes).forEach((attr) => {
          if (
            !attr.name.startsWith("sodipodi:") &&
            !(
              newElement.tagName === "circle" &&
              (attr.name === "cx" || attr.name === "cy" || attr.name === "r")
            ) &&
            !(newElement.tagName === "path" && attr.name === "d")
          ) {
            newElement.setAttribute(attr.name, attr.value);
          }
        });

        arc.parentNode.replaceChild(newElement, arc);
      } catch (error) {
        console.warn("Failed to convert sodipodi arc:", error);
        // Even if conversion fails, remove sodipodi attributes to clean up the original element
        Array.from(arc.attributes).forEach((attr) => {
          if (attr.name.startsWith("sodipodi:")) {
            arc.removeAttribute(attr.name);
          }
        });
      }
    });

    return new XMLSerializer().serializeToString(doc);
  }

  createEllipticalArcPath(
    cx: number,
    cy: number,
    rx: number,
    ry: number,
    startAngle: number,
    endAngle: number,
  ) {
    // Convert polar coordinates to Cartesian for start and end points
    const startX = cx + rx * Math.cos(startAngle);
    const startY = cy + ry * Math.sin(startAngle);
    const endX = cx + rx * Math.cos(endAngle);
    const endY = cy + ry * Math.sin(endAngle);

    // Calculate the difference in angles to determine large-arc-flag and sweep-flag
    // The direction for sodipodi is usually positive (counter-clockwise on standard cartesian plane,
    // which translates to clockwise in SVG's Y-down coordinate system if sweep-flag is 1).
    let angleDiff = endAngle - startAngle;
    if (angleDiff < 0) {
      angleDiff += 2 * Math.PI; // Normalize angle difference to be positive
    }

    // large-arc-flag: 1 if angleDiff > PI, 0 otherwise
    const largeArcFlag = angleDiff > Math.PI ? 1 : 0;

    // sweep-flag: 1 for clockwise (positive angle increase), 0 for counter-clockwise.
    // Given sodipodi's positive angle convention for 'end' relative to 'start',
    // and SVG's default y-axis going down, a positive angle increase in sodipodi
    // usually means a clockwise sweep in SVG for the most direct path.
    // If endAngle < startAngle initially, it implies a counter-clockwise sweep.
    // However, after normalization, we need to be careful.
    // The simplest way to think about it for sodipodi's convention:
    // If endAngle is 'after' startAngle in the positive direction (0 to 2PI), sweepFlag should be 1.
    // If endAngle is 'before' startAngle (meaning it wrapped around), sweepFlag should be 0.
    // After normalization, if endAngle > startAngle, we assume positive sweep (clockwise for SVG).
    // If startAngle > endAngle, it means we crossed the 0/2PI boundary, implying a "negative" sweep,
    // which in SVG typically means `sweepFlag=0` if `endAngle` is reached by going CCW from `startAngle`.

    // A simpler heuristic for sweepFlag, assuming sodipodi's 'start' and 'end' define the arc
    // in a positive (counter-clockwise) direction on a standard mathematical coordinate system.
    // SVG's Y-axis is inverted. So, a counter-clockwise arc in math becomes a clockwise arc in SVG
    // if sweep-flag is 1.
    const sweepFlag = 1; // Assuming positive angle rotation from sodipodi translates to clockwise in SVG

    return `M ${startX} ${startY} A ${rx} ${ry} 0 ${largeArcFlag} ${sweepFlag} ${endX} ${endY}`;
  }

  // Helper to parse SVG length values (e.g., "10mm", "20px", "50")
  parseSvgLength(lengthStr: string | null, defaultVal = 0): number {
    if (typeof lengthStr !== "string") return defaultVal;

    const match = lengthStr.match(/^([\d.]+)([a-z]*)$/i);
    if (match) {
      const value = parseFloat(match[1]);
      const unit = match[2].toLowerCase() || "px"; // Default to px if no unit

      if (this.unitConversion[unit]) {
        return value * this.unitConversion[unit];
      } else {
        // For unknown units, assume pixels if no unit is specified (e.g., "100")
        // If a unit is specified but unknown, it's safer to default to 0 or log a warning.
        console.warn(
          `Unknown SVG unit: "${unit}" in "${lengthStr}". Treating value as pixels.`,
        );
        return value; // Treat as pixels if unit is unrecognized
      }
    }
    // If no unit or conversion factor, just try to parse as float (plain number)
    return parseFloat(lengthStr) || defaultVal;
  }

  /**
   * Calculates the bounding box of the SVG content within its current coordinate system.
   * This method requires a browser environment to function correctly as it relies on
   * SVG DOM methods like `getBBox()`.
   * @param {string} svgString The SVG content as a string.
   * @returns {object|null} An object with {x, y, width, height} or null if calculation fails.
   */
  calculateContentBBox(
    svgString: string,
  ): { x: number; y: number; width: number; height: number } | null {
    // Create a temporary SVG element in a detached DOM fragment or hidden div
    const tempDiv = document.createElement("div");
    tempDiv.style.position = "absolute";
    tempDiv.style.top = "-9999px";
    tempDiv.style.left = "-9999px";
    tempDiv.style.width = "0";
    tempDiv.style.height = "0";
    tempDiv.style.overflow = "hidden";
    document.body.appendChild(tempDiv); // Append to body to make getBBox work

    tempDiv.innerHTML = sanitizePreviewSvg(svgString);
    const svgElement = tempDiv.querySelector("svg");

    if (!svgElement) {
      document.body.removeChild(tempDiv);
      console.error(
        "No SVG element found in the provided string for BBox calculation.",
      );
      return null;
    }

    try {
      const calculateMarkerPadding = (): number => {
        const markerAttrs = ["marker-start", "marker-mid", "marker-end"];
        const markerElements = Array.from(
          svgElement.querySelectorAll("marker[id]"),
        );
        let padding = 0;

        svgElement
          .querySelectorAll(
            markerAttrs.map((attr) => `[${attr}]`).join(","),
          )
          .forEach((referencingElement) => {
            markerAttrs.forEach((attr) => {
              const markerRef = referencingElement.getAttribute(attr);
              const markerId = markerRef?.match(
                /^url\(\s*['"]?#([^'")\s]+)['"]?\s*\)$/,
              )?.[1];
              if (!markerId) return;

              const marker = markerElements.find(
                (candidate) => candidate.getAttribute("id") === markerId,
              );
              if (!marker) return;

              const markerContent = document.createElementNS(
                "http://www.w3.org/2000/svg",
                "g",
              );
              markerContent.setAttribute("data-autocrop-marker-content", "");
              Array.from(marker.children).forEach((child) => {
                markerContent.appendChild(child.cloneNode(true));
              });
              svgElement.appendChild(markerContent);

              let markerBBox: DOMRect;
              try {
                markerBBox = markerContent.getBBox();
              } finally {
                markerContent.remove();
              }

              const viewBox = (marker.getAttribute("viewBox") || "")
                .trim()
                .split(/[\s,]+/)
                .map(Number);
              const hasViewBox =
                viewBox.length === 4 &&
                viewBox.every(Number.isFinite) &&
                viewBox[2] > 0 &&
                viewBox[3] > 0;
              const markerWidth =
                this.parseSvgLength(marker.getAttribute("markerWidth"), 3) ||
                3;
              const markerHeight =
                this.parseSvgLength(marker.getAttribute("markerHeight"), 3) ||
                3;

              let scaleX = 1;
              let scaleY = 1;
              let refX = parseFloat(marker.getAttribute("refX") || "0") || 0;
              let refY = parseFloat(marker.getAttribute("refY") || "0") || 0;
              if (hasViewBox) {
                scaleX = markerWidth / viewBox[2];
                scaleY = markerHeight / viewBox[3];
                const preserveAspectRatio =
                  marker.getAttribute("preserveAspectRatio") || "xMidYMid meet";
                if (!/\bnone\b/.test(preserveAspectRatio)) {
                  const uniformScale = /\bslice\b/.test(preserveAspectRatio)
                    ? Math.max(scaleX, scaleY)
                    : Math.min(scaleX, scaleY);
                  scaleX = uniformScale;
                  scaleY = uniformScale;
                }
              }

              const markerUnitScale =
                marker.getAttribute("markerUnits") === "userSpaceOnUse"
                  ? 1
                  : parseFloat(
                      referencingElement.getAttribute("stroke-width") ||
                        getComputedStyle(referencingElement).strokeWidth ||
                        "1",
                    ) || 1;
              const maxXDistance = Math.max(
                Math.abs(markerBBox.x - refX),
                Math.abs(markerBBox.x + markerBBox.width - refX),
              );
              const maxYDistance = Math.max(
                Math.abs(markerBBox.y - refY),
                Math.abs(markerBBox.y + markerBBox.height - refY),
              );
              const markerRadius = Math.hypot(
                maxXDistance * scaleX,
                maxYDistance * scaleY,
              );
              padding = Math.max(padding, markerRadius * markerUnitScale);
            });
          });

        return padding;
      };

      // Get the bounding box of the entire SVG content
      // This method accounts for all visible elements.
      // It returns an SVGRect which has x, y, width, height.
      // Important: This can fail or return 0,0,0,0 if the SVG is empty or has no renderable content.
      const bbox = svgElement.getBBox();

      // Check if bbox is valid (not empty)
      if (
        bbox.width === 0 &&
        bbox.height === 0 &&
        bbox.x === 0 &&
        bbox.y === 0
      ) {
        // This might indicate an empty SVG or elements with no rendering area.
        // Fallback to parsing viewBox or width/height if getBBox is effectively zero.
        // Or, if content is truly empty, return null.
        console.warn(
          "getBBox returned an empty bounding box. Trying to derive from SVG attributes.",
        );
        let currentWidth,
          currentHeight,
          minX = 0,
          minY = 0;
        let viewBoxAttr = svgElement.getAttribute("viewBox");
        if (viewBoxAttr) {
          const vbParts = viewBoxAttr.split(/\s+/).map(Number);
          minX = vbParts[0] || 0;
          minY = vbParts[1] || 0;
          currentWidth = vbParts[2] || 0;
          currentHeight = vbParts[3] || 0;
        } else {
          currentWidth = this.parseSvgLength(
            svgElement.getAttribute("width"),
            0,
          );
          currentHeight = this.parseSvgLength(
            svgElement.getAttribute("height"),
            0,
          );
        }

        if (currentWidth > 0 && currentHeight > 0) {
          return {
            x: minX,
            y: minY,
            width: currentWidth,
            height: currentHeight,
          };
        } else {
          console.error(
            "Could not determine content bounding box from getBBox or SVG attributes.",
          );
          return null;
        }
      }

      const markerPadding = calculateMarkerPadding();
      return {
        x: bbox.x - markerPadding,
        y: bbox.y - markerPadding,
        width: bbox.width + markerPadding * 2,
        height: bbox.height + markerPadding * 2,
      };
    } catch (e) {
      console.error("Error calculating SVG bounding box:", e);
      return null;
    } finally {
      document.body.removeChild(tempDiv); // Clean up the temporary element
    }
  }

  /**
   * Resizes an SVG by adjusting its viewBox and width/height attributes
   * to fit the content into a new canvas size while preserving aspect ratio.
   * @param {string} svg The SVG content as a string.
   * @param {number} newWidth The desired new width in pixels.
   * @param {number} newHeight The desired new height in pixels.
   * @returns {string} The resized SVG string.
   */
  resizeSvg(svg: string, newWidth: number, newHeight: number): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");
    const svgElement = doc.documentElement;

    if (svgElement.tagName !== "svg") {
      console.error(
        "The provided SVG string does not contain an SVG element as its root.",
      );
      return svg;
    }

    if (!Number.isFinite(newWidth) || !Number.isFinite(newHeight)) {
      return svg;
    }

    const contentBBox = this.calculateContentBBox(svg);

    let finalViewBoxX, finalViewBoxY, finalViewBoxWidth, finalViewBoxHeight;

    if (contentBBox && contentBBox.width > 0 && contentBBox.height > 0) {
      // Apply flooring to x and y, and ceiling to the max x/y to ensure full visibility
      const x1 = Math.floor(contentBBox.x);
      const y1 = Math.floor(contentBBox.y);
      const x2 = Math.ceil(contentBBox.x + contentBBox.width);
      const y2 = Math.ceil(contentBBox.y + contentBBox.height);

      finalViewBoxX = x1;
      finalViewBoxY = y1;
      finalViewBoxWidth = x2 - x1;
      finalViewBoxHeight = y2 - y1;

      // Handle potential edge case where width/height might become 0 after rounding
      if (finalViewBoxWidth === 0 && contentBBox.width > 0) {
        finalViewBoxWidth = 1;
      }
      if (finalViewBoxHeight === 0 && contentBBox.height > 0) {
        finalViewBoxHeight = 1;
      }
    } else {
      console.warn(
        "Could not calculate content BBox reliably. Falling back to existing viewBox or a default.",
      );
      let currentWidth,
        currentHeight,
        minX = 0,
        minY = 0;
      let viewBoxAttr = svgElement.getAttribute("viewBox");

      if (viewBoxAttr) {
        const vbParts = viewBoxAttr.split(/\s+/).map(Number);
        minX = Math.floor(vbParts[0] || 0);
        minY = Math.floor(vbParts[1] || 0);
        currentWidth = Math.ceil(vbParts[2] || 0); // Round up width/height
        currentHeight = Math.ceil(vbParts[3] || 0);
      } else {
        minX = 0;
        minY = 0;
        currentWidth = Math.ceil(
          this.parseSvgLength(svgElement.getAttribute("width"), 100),
        );
        currentHeight = Math.ceil(
          this.parseSvgLength(svgElement.getAttribute("height"), 100),
        );
      }

      finalViewBoxX = minX;
      finalViewBoxY = minY;
      finalViewBoxWidth = currentWidth;
      finalViewBoxHeight = currentHeight;

      if (finalViewBoxWidth <= 0 || finalViewBoxHeight <= 0) {
        console.error(
          "Failed to determine a valid content area for resizing. Using a default 0 0 100 100 viewBox.",
        );
        finalViewBoxX = 0;
        finalViewBoxY = 0;
        finalViewBoxWidth = 100;
        finalViewBoxHeight = 100;
      }
    }

    // Set the new width and height for the SVG element
    svgElement.setAttribute("width", newWidth.toString());
    svgElement.setAttribute("height", newHeight.toString());

    // Set the viewBox with integer coordinates
    svgElement.setAttribute(
      "viewBox",
      `${finalViewBoxX} ${finalViewBoxY} ${finalViewBoxWidth} ${finalViewBoxHeight}`,
    );

    // preserveAspectRatio remains default 'xMidyMid meet' unless specified otherwise.

    return new XMLSerializer().serializeToString(doc);
  }

  /**
   * Crops the SVG viewBox to its content bounds with a margin.
   * @param {string} svg The SVG content as a string.
   * @param {number} margin The margin (in user units) to add around the content.
   * @returns {string} The cropped SVG string.
   */
  autocropSvg(svg: string, margin = 3): string {
    const parser = new DOMParser();
    const doc = parser.parseFromString(svg, "image/svg+xml");
    const svgElement = doc.documentElement;

    if (svgElement.tagName !== "svg") {
      console.error(
        "The provided SVG string does not contain an SVG element as its root.",
      );
      return svg;
    }

    const contentBBox = this.calculateContentBBox(svg);
    if (
      !contentBBox ||
      (contentBBox.width <= 0 && contentBBox.height <= 0)
    ) {
      console.warn(
        "Could not calculate content BBox for autocrop. Leaving SVG unchanged.",
      );
      return svg;
    }

    const safeMargin = Number.isFinite(margin) ? margin : 0;
    const x1 = Math.floor(contentBBox.x - safeMargin);
    const y1 = Math.floor(contentBBox.y - safeMargin);
    const x2 = Math.ceil(contentBBox.x + contentBBox.width + safeMargin);
    const y2 = Math.ceil(contentBBox.y + contentBBox.height + safeMargin);

    let width = x2 - x1;
    let height = y2 - y1;

    if (width <= 0) width = 1;
    if (height <= 0) height = 1;

    svgElement.setAttribute("viewBox", `${x1} ${y1} ${width} ${height}`);
    return new XMLSerializer().serializeToString(doc);
  }

  sanitizeSvg(svg: string): string {
    if (!svg || typeof svg !== "string") return svg;

    try {
      // Try to parse and validate the SVG
      const parser = new DOMParser();
      const doc = parser.parseFromString(svg, "image/svg+xml");
      const parseError = doc.querySelector("parsererror");
      if (parseError) {
        console.warn("SVG parsing error:", parseError.textContent);
        // Try to fix common issues
        svg = this.fixCommonSvgIssues(svg);
      }
    } catch (e) {
      console.warn("SVG sanitization failed, using original:", e);
    }
    return svg;
  }

  fixCommonSvgIssues(svg: string): string {
    // Try to fix common SVG issues
    let fixed = svg;

    // Fix malformed path data - handle missing numbers after operators
    // Pattern: ...124 6.8 18-12.51Z (where 18-12.51 should be separate numbers)
    fixed = fixed.replace(/(\d+)-(\d+(\.\d+)?)/g, "$1 $2");

    // Fix common encoding issues
    fixed = fixed.replace(/&gt;/g, ">");
    fixed = fixed.replace(/&lt;/g, "<");
    fixed = fixed.replace(/&amp;/g, "&");
    fixed = fixed.replace(/&quot;/g, '"');
    fixed = fixed.replace(/&apos;/g, "'");

    // Fix common whitespace issues
    fixed = fixed.replace(/>\s+</g, "><");

    return fixed;
  }

  updateTreeDoc(): void {
    const sourceSvg = this.getSourceSvg();
    if (!sourceSvg) {
      this.options.treeDoc = null;
      return;
    }
    const parser = new DOMParser();
    // Tree view reflects the editable source SVG
    const svgToParse = this.normalizeNamespaces(sourceSvg);
    this.options.treeDoc = parser.parseFromString(svgToParse, "image/svg+xml");
  }

  async optimizeSvg(): Promise<void> {
    const sourceSvg = this.getSourceSvg();
    if (!sourceSvg || !sourceSvg.trim() || !/<svg\b/i.test(sourceSvg)) {
      this.optimizedSvg = '<svg xmlns="http://www.w3.org/2000/svg"></svg>';
      return;
    }

    if (!this.isOptimizationEnabled()) {
      this.optimizedSvg = this.normalizeNamespaces(sourceSvg);
      return;
    }

    try {
      let svg = this.normalizeNamespaces(sourceSvg);
      const preservedResult = this.injectPreserveMarkers(svg);
      svg = preservedResult.svg;

      // Convert sodipodi arcs BEFORE SVGO to prevent removal
      if (this.options.convertSodipodiArcs) {
        svg = this.convertSodipodiArcs(svg);
      }

      // Protect marker paths from SVGO's mergePaths
      svg = this.tagMarkerPaths(svg);

      // First pass with SVGO
      const svgoResult = optimize(svg, {
        plugins: [
          "preset-default",
          "removeDoctype",
          "removeXMLProcInst",
          "removeComments",
          "removeMetadata",
          "removeTitle",
          "removeDesc",
          "removeUselessDefs",
          "removeEditorsNSData",
          "removeEmptyAttrs",
          "removeHiddenElems",
          "removeEmptyText",
          "removeEmptyContainers",
          "removeViewBox",
          "cleanupEnableBackground",
          "convertStyleToAttrs",
          "convertPathData",
          {
            name: "removeUnknownsAndDefaults",
            params: {
              keepDataAttrs: true,
              keepAriaAttrs: true,
              keepRoleAttr: true,
            },
          },
          "removeNonInheritableGroupAttrs",
          "removeUnusedNS",
          "cleanupIds",
          "cleanupNumericValues",
          "moveElemsAttrsToGroup",
          "moveGroupAttrsToElems",
          "collapseGroups",
          "removeRasterImages",
          "mergePaths",
          "convertShapeToPath",
          "sortAttrs",
          "removeDimensions",
        ],
      });

      svg = svgoResult.data;
      svg = this.fixInvalidHexColors(svg);
      svg = this.untagMarkerPaths(svg);
      svg = this.normalizeXlinkHrefs(svg);
      svg = this.convertXlinkHrefs(svg);
      svg = this.stripUndeclaredNamespacedContent(svg);

      // Apply custom optimizations
      if (this.options.precision >= 0) {
        svg = collapseTransforms(svg);
        svg = this.roundNumbersWithPrecision(
          svg,
          this.options.precision,
          this.options.pathPrecision,
        );
      } else {
        svg = collapseTransforms(svg);
      }

      if (this.options.removeDefaultValues) {
        svg = this.removeDefaultValues(svg);
      }

      if (this.options.trimText) {
        svg = this.trimTextContent(svg);
      }

      if (this.options.removeFontFamily || this.options.removeFontSize) {
        svg = this.removeFontAttributes(
          svg,
          this.options.removeFontFamily,
          this.options.removeFontSize,
        );
      }

      if (this.options.removeTspan) {
        svg = this.removeTspanElements(svg);
      }

      if (this.options.removeStyling) {
        svg = this.removeStyling(svg);
      }

      if (this.options.groupTextElementsAtEnd) {
        svg = this.moveTextElementsToEnd(svg);
      }

      if (this.options.groupSimilarElements) {
        // Group similar elements only (disable aggressive path combining for now)
        svg = this.groupSimilarElementsByType(svg);
        // Note: combinePaths disabled as it's too aggressive
        // svg = this.combinePaths(svg);
      }

      // Remove duplicate definitions
      svg = this.removeDuplicateDefs(svg);

      // Remove stroke attributes from text elements
      svg = this.removeStrokeFromText(svg);

      // Merge simple path-only groups and collapse groups with a single child
      svg = this.mergePathsAndCollapseGroups(svg);

      // Remove unused xlink namespace if possible
      svg = this.removeUnusedXlinkNamespace(svg);

      // Remove default root attributes like version="1.1"
      svg = this.removeSvgRootDefaults(svg);

      if (this.options.autoAutocrop) {
        svg = this.autocropSvg(svg, 3);
      }

      if (this.options.useCustomDimensions) {
        svg = this.resizeSvg(
          svg,
          this.options.customWidth,
          this.options.customHeight,
        );
      }

      svg = this.restorePreservedAttributes(svg, preservedResult.preserved);
      svg = this.removeDanglingNamespacedAttributes(svg);
      svg = this.fixInvalidHexColors(svg);

      this.optimizedSvg = svg;
    } catch (error) {
      console.error("Optimization error:", error);
      this.optimizedSvg = `<!-- Error optimizing SVG: ${error.message} -->\n${sourceSvg}`;
    }
  }

  async downloadSvg(): Promise<void> {
    const svg = this.getPreviewSvg();
    if (!svg) return;

    const blob = new Blob([svg], { type: "image/svg+xml" });

    if (typeof window.showSaveFilePicker === "function") {
      try {
        const handle = await window.showSaveFilePicker({
          suggestedName: "optimized.svg",
          types: [
            { description: "SVG image", accept: { "image/svg+xml": [".svg"] } },
          ],
        });
        const writable = await handle.createWritable();
        await writable.write(blob);
        await writable.close();
        return;
      } catch (error) {
        // AbortError means the user cancelled the picker — do nothing.
        if ((error as { name?: string })?.name === "AbortError") return;
        console.warn("showSaveFilePicker failed, falling back to download:", error);
      }
    }

    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = "optimized.svg";
    document.body.appendChild(a);
    a.click();
    document.body.removeChild(a);
    URL.revokeObjectURL(url);
  }

  getStats(): {
    originalSize: number;
    optimizedSize: number;
    reduction: number;
    reductionPercent: number;
  } {
    const sourceSvg = this.getSourceSvg() || "";
    const previewSvg = this.getPreviewSvg() || "";
    const originalSize = new Blob([sourceSvg]).size;
    const optimizedSize = new Blob([previewSvg]).size;
    const reduction = originalSize - optimizedSize;
    const reductionPercent =
      originalSize > 0 ? (reduction / originalSize) * 100 : 0;

    return {
      originalSize,
      optimizedSize,
      reduction,
      reductionPercent,
    };
  }

  formatBytes(bytes: number) {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
  }

  // History management methods
  saveToHistory(): void {
    if (this.isRestoringHistory) return;

    if (this.historyPointer < this.history.length - 1) {
      this.history = this.history.slice(0, this.historyPointer + 1);
    }

    this.history.push({
      originalSvg: this.originalSvg,
      optimizedSvg: this.optimizedSvg,
      options: { ...this.options },
    });

    if (this.history.length > this.maxHistory) {
      this.history.shift();
    }

    this.historyPointer = this.history.length - 1;
    this.persistState();
    this.notifyVscode();
    m.redraw();
  }

  notifyVscode(): void {
    if (!vscodeApi) return;
    vscodeApi.postMessage({
      type: "update-svg",
      svg: this.getSourceSvg() || "",
      optimizedSvg: this.getPreviewSvg() || "",
    });
  }

  canUndo(): boolean {
    return this.historyPointer > 0;
  }

  canRedo(): boolean {
    return this.historyPointer < this.history.length - 1;
  }

  undo(): void {
    if (!this.canUndo()) return;

    this.isRestoringHistory = true;
    this.historyPointer--;

    const state = this.history[this.historyPointer];
    this.originalSvg = state.originalSvg;
    this.optimizedSvg = state.optimizedSvg;
    this.options = { ...state.options };

    if (this.editor) {
      this.editor.setValue(this.originalSvg);
    }

    this.updateTreeDoc();
    this.optimizeSvg();
    this.persistState();
    this.isRestoringHistory = false;
    m.redraw();
  }

  redo(): void {
    if (!this.canRedo()) return;

    this.isRestoringHistory = true;
    this.historyPointer++;

    const state = this.history[this.historyPointer];
    this.originalSvg = state.originalSvg;
    this.optimizedSvg = state.optimizedSvg;
    this.options = { ...state.options };

    if (this.editor) {
      this.editor.setValue(this.originalSvg);
    }

    this.updateTreeDoc();
    this.optimizeSvg();
    this.persistState();
    this.isRestoringHistory = false;
    m.redraw();
  }
}

export const optimizer = new SVGOptimizer();
export type { OptimizeOptions, HistoryEntry };
export { SVGOptimizer };
