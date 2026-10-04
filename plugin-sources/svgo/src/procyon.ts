import type { ProcyonOptimizerSettings, SVGOptimizer } from "./optimizer";

type ProcyonPlugin = {
  loadToken: string;
  theme?: "light" | "dark";
  settings?: Partial<ProcyonOptimizerSettings>;
  postMessage(message:
    | { type: "save-svg"; svg: string }
    | { type: "settings-change"; settings: ProcyonOptimizerSettings; sequence: number; flush?: true }
  ): void;
};

export const procyonPlugin: ProcyonPlugin | undefined =
  typeof window !== "undefined" &&
  typeof window.procyonPlugin?.postMessage === "function" &&
  typeof window.procyonPlugin.loadToken === "string" &&
  window.procyonPlugin.loadToken.length > 0
    ? window.procyonPlugin
    : undefined;

let documentLoaded = false;

export type ProcyonSaveResult =
  | { type: "save-result"; success: true }
  | { type: "save-result"; success: false; error?: string };
export type ProcyonSettingsResult =
  | { type: "settings-result"; success: true }
  | { type: "settings-result"; success: false; error?: string };
export type ProcyonThemeChange = { type: "theme-change"; theme: "light" | "dark" };

export function handleProcyonMessage(
  event: MessageEvent,
  optimizer: SVGOptimizer,
): { type: "load-svg" } | { type: "flush-settings" } | ProcyonSaveResult | ProcyonSettingsResult | ProcyonThemeChange | null {
  if (!procyonPlugin || event.source !== window) return null;
  const data = event.data;
  if (
    !data ||
    typeof data !== "object" ||
    data.loadToken !== procyonPlugin.loadToken
  ) return null;
  if (data.type === "load-svg") {
    if (typeof data.svg !== "string" || typeof data.uri !== "string") return null;
    documentLoaded = true;
    optimizer.loadSvgString(data.svg);
    return { type: "load-svg" };
  }
  if (data.type === "theme-change" && (data.theme === "light" || data.theme === "dark")) {
    return { type: "theme-change", theme: data.theme };
  }
  if (data.type === "flush-settings") {
    optimizer.flushProcyonSettings();
    return { type: "flush-settings" };
  }
  if (
    data.type === "save-result" &&
    typeof data.success === "boolean" &&
    (data.error === undefined || typeof data.error === "string")
  ) {
    return data.success
      ? { type: "save-result", success: true }
      : { type: "save-result", success: false, error: data.error };
  }
  if (
    data.type === "settings-result" &&
    typeof data.success === "boolean" &&
    (data.error === undefined || typeof data.error === "string") &&
    typeof data.sequence === "number" &&
    Number.isSafeInteger(data.sequence) &&
    data.sequence > 0
  ) {
    if (!optimizer.acknowledgeProcyonSettings(data.success, data.sequence)) return null;
    return data.success
      ? { type: "settings-result", success: true }
      : { type: "settings-result", success: false, error: data.error };
  }
  return null;
}

export function canSaveProcyonSvg(optimizer: SVGOptimizer): boolean {
  return documentLoaded && Boolean(optimizer.getSourceSvg().trim());
}

export function saveProcyonSvg(optimizer: SVGOptimizer): void {
  if (!procyonPlugin || !canSaveProcyonSvg(optimizer)) return;
  procyonPlugin.postMessage({ type: "save-svg", svg: optimizer.getSourceSvg() });
}
