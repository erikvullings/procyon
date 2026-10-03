// @vitest-environment jsdom
import m from "mithril";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SVGOptimizer } from "../src/optimizer";
import { Sidebar } from "../src/components/sidebar";
import { Header } from "../src/components/header";

describe("Procyon host adapter", () => {
  const postMessage = vi.fn();
  const svg = '<svg xmlns="http://www.w3.org/2000/svg"><rect width="10"/></svg>';

  beforeEach(() => {
    vi.resetModules();
    postMessage.mockReset();
    window.procyonPlugin = { loadToken: "test-window-token", postMessage };
    document.body.replaceChildren();
  });

  it("accepts only same-window loads with the matching token and valid payload", async () => {
    const { handleProcyonMessage } = await import("../src/procyon");
    const optimizer = new SVGOptimizer();
    const dispatch = (data: unknown, source: MessageEventSource | null = window) =>
      handleProcyonMessage(new MessageEvent("message", { data, source }), optimizer);

    dispatch({ type: "load-svg", svg, uri: "file:///image.svg", loadToken: "test-window-token" }, null);
    dispatch({ type: "load-svg", svg, uri: "file:///image.svg", loadToken: "wrong" });
    dispatch({ type: "load-svg", svg, uri: "file:///image.svg" });
    dispatch({ type: "load-svg", svg: 1, uri: "file:///image.svg", loadToken: "test-window-token" });
    expect(optimizer.getSourceSvg()).toBe("");

    dispatch({ type: "load-svg", svg, uri: "file:///image.svg", loadToken: "test-window-token" });
    expect(optimizer.getSourceSvg()).toBe(svg);
    expect(postMessage).not.toHaveBeenCalled();

    optimizer.loadSvgString('<svg xmlns="http://www.w3.org/2000/svg"><circle r="5"/></svg>');
    expect(postMessage).not.toHaveBeenCalled();
  });

  it("sends the current source exactly once per explicit save and never the preview", async () => {
    const { handleProcyonMessage, saveProcyonSvg } = await import("../src/procyon");
    const optimizer = new SVGOptimizer();
    saveProcyonSvg(optimizer);
    expect(postMessage).not.toHaveBeenCalled();

    optimizer.loadSvgString(svg);
    saveProcyonSvg(optimizer);
    expect(postMessage).not.toHaveBeenCalled();
    handleProcyonMessage(new MessageEvent("message", {
      data: { type: "load-svg", svg, uri: "file:///image.svg", loadToken: "test-window-token" },
      source: window,
    }), optimizer);
    optimizer.optimizedSvg = "<svg/>";
    saveProcyonSvg(optimizer);
    expect(postMessage).toHaveBeenCalledExactlyOnceWith({ type: "save-svg", svg });
  });

  it("places Save in the header and Copy in the menu only with the host", async () => {
    const { canSaveProcyonSvg, handleProcyonMessage, saveProcyonSvg } = await import("../src/procyon");
    const optimizer = new SVGOptimizer();
    const copy = vi.fn();
    const sidebarAttrs = {
      optimizer,
      sourceSvg: svg,
      theme: "dark" as const,
      onToggleTheme: () => {},
      open: true,
      showFileActions: false,
      showDownload: false,
      onCopy: copy,
      isCopied: false,
    };
    const headerAttrs = {
      stats: { originalSizeLabel: "0 B", optimizedSizeLabel: "0 B", reductionLabel: "0 B", reductionClass: "" },
      showTitle: false,
      onToggleSidebar: () => {},
      canOptimize: false,
      onOptimize: () => {},
      canCopy: true,
      isCopied: false,
      onCopy: copy,
      onSave: () => saveProcyonSvg(optimizer),
      canSave: canSaveProcyonSvg(optimizer),
    };
    m.render(document.body, [m(Sidebar, sidebarAttrs), m(Header, headerAttrs)]);
    expect(document.querySelector('button[title="Download optimized SVG"]')).toBeNull();
    expect(document.querySelector(".sidebar [title='Save SVG to Procyon']")).toBeNull();
    expect(document.querySelector(".header [title='Copy source SVG to clipboard']")).toBeNull();
    document.querySelector<HTMLButtonElement>(".sidebar [title='Copy source SVG to clipboard']")?.click();
    expect(copy).toHaveBeenCalledOnce();
    let save = document.querySelector<HTMLButtonElement>('button[title="Save SVG to Procyon"]');
    expect(save?.disabled).toBe(true);
    handleProcyonMessage(new MessageEvent("message", {
      data: { type: "load-svg", svg, uri: "file:///image.svg", loadToken: "test-window-token" },
      source: window,
    }), optimizer);
    m.render(document.body, [
      m(Sidebar, sidebarAttrs),
      m(Header, { ...headerAttrs, canSave: canSaveProcyonSvg(optimizer) }),
    ]);
    save = document.querySelector<HTMLButtonElement>('button[title="Save SVG to Procyon"]');
    expect(save?.disabled).toBe(false);
    save?.click();
    expect(postMessage).toHaveBeenCalledExactlyOnceWith({ type: "save-svg", svg });

    m.render(document.body, [
      m(Sidebar, { ...sidebarAttrs, onCopy: undefined, showDownload: true }),
      m(Header, { ...headerAttrs, onSave: undefined, showTitle: true }),
    ]);
    expect(document.querySelector('button[title="Save SVG to Procyon"]')).toBeNull();
    expect(document.querySelector('button[title="Download optimized SVG"]')).not.toBeNull();
    expect(document.querySelector(".header [title='Copy source SVG to clipboard']")).not.toBeNull();
  });

  it("handles Cmd/Ctrl+S even while the code editor or an input is focused", async () => {
    const { optimizer } = await import("../src/optimizer");
    const { handleProcyonMessage } = await import("../src/procyon");
    const { initializeGlobalHandlers } = await import("../src/ui");
    initializeGlobalHandlers();

    const editor = document.createElement("div");
    editor.id = "editor";
    document.body.append(editor);
    editor.dispatchEvent(new KeyboardEvent("keydown", {
      key: "s", metaKey: true, bubbles: true, cancelable: true,
    }));
    expect(postMessage).not.toHaveBeenCalled();
    handleProcyonMessage(new MessageEvent("message", {
      data: { type: "load-svg", svg, uri: "file:///image.svg", loadToken: "test-window-token" },
      source: window,
    }), optimizer);
    editor.dispatchEvent(new KeyboardEvent("keydown", {
      key: "s", metaKey: true, bubbles: true, cancelable: true,
    }));
    expect(postMessage).toHaveBeenCalledExactlyOnceWith({ type: "save-svg", svg });

    const input = document.createElement("input");
    document.body.append(input);
    const shortcut = new KeyboardEvent("keydown", {
      key: "s", ctrlKey: true, bubbles: true, cancelable: true,
    });
    input.dispatchEvent(shortcut);
    expect(shortcut.defaultPrevented).toBe(true);
    expect(postMessage).toHaveBeenCalledTimes(2);
  });

  it("preserves automatic VS Code updates without the Procyon bridge", async () => {
    delete window.procyonPlugin;
    const vscodePostMessage = vi.fn();
    vi.stubGlobal("acquireVsCodeApi", () => ({ postMessage: vscodePostMessage }));
    vi.resetModules();
    try {
      const { SVGOptimizer: VscodeOptimizer } = await import("../src/optimizer");
      const optimizer = new VscodeOptimizer();
      vscodePostMessage.mockClear();
      optimizer.loadSvgString(svg);
      expect(vscodePostMessage).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
        type: "update-svg",
        svg,
      }));
      expect(postMessage).not.toHaveBeenCalled();
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("accepts only typed save results from the trusted window and token", async () => {
    const { handleProcyonMessage } = await import("../src/procyon");
    const optimizer = new SVGOptimizer();
    const result = (data: unknown, source: MessageEventSource | null = window) =>
      handleProcyonMessage(new MessageEvent("message", { data, source }), optimizer);

    expect(result({ type: "save-result", success: false, error: "Disk full", loadToken: "test-window-token" }, null)).toBeNull();
    expect(result({ type: "save-result", success: false, error: "Disk full", loadToken: "wrong" })).toBeNull();
    expect(result({ type: "save-result", success: "false", error: "Disk full", loadToken: "test-window-token" })).toBeNull();
    expect(result({ type: "save-result", success: false, error: 42, loadToken: "test-window-token" })).toBeNull();
    expect(result({ type: "save-result", success: false, error: "Disk full", loadToken: "test-window-token" })).toEqual({
      type: "save-result", success: false, error: "Disk full",
    });
    expect(result({ type: "save-result", success: false, loadToken: "test-window-token" })).toEqual({
      type: "save-result", success: false, error: undefined,
    });
    expect(result({ type: "save-result", success: true, loadToken: "test-window-token" })).toEqual({
      type: "save-result", success: true,
    });
    expect(result({ type: "theme-change", theme: "light", loadToken: "test-window-token" }, null)).toBeNull();
    expect(result({ type: "theme-change", theme: "light", loadToken: "wrong" })).toBeNull();
    expect(result({ type: "theme-change", theme: "auto", loadToken: "test-window-token" })).toBeNull();
    expect(result({ type: "theme-change", theme: "light", loadToken: "test-window-token" })).toEqual({
      type: "theme-change", theme: "light",
    });
  });

  it("serializes preference changes and flushes the latest snapshot before close", async () => {
    const { SVGOptimizer: HostOptimizer } = await import("../src/optimizer");
    const { handleProcyonMessage } = await import("../src/procyon");
    const optimizer = new HostOptimizer();
    const receive = (data: unknown, source: MessageEventSource | null = window) =>
      handleProcyonMessage(new MessageEvent("message", { data, source }), optimizer);
    expect(postMessage).not.toHaveBeenCalled();
    optimizer.options.precision = 3;
    optimizer.persistSessionState();
    expect(postMessage).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({
      type: "settings-change", sequence: 1, settings: expect.objectContaining({ precision: 3 }),
    }));
    optimizer.options.precision = 4;
    optimizer.persistSessionState();
    optimizer.options.precision = 5;
    optimizer.persistSessionState();
    expect(postMessage).toHaveBeenCalledTimes(1);
    expect(receive({ type: "settings-result", success: true, sequence: 1, loadToken: "wrong" })).toBeNull();
    expect(receive({ type: "settings-result", success: true, sequence: 1, loadToken: "test-window-token" }, null)).toBeNull();
    expect(postMessage).toHaveBeenCalledTimes(1);
    expect(receive({ type: "settings-result", success: true, sequence: 1, loadToken: "test-window-token" })).toEqual({
      type: "settings-result", success: true,
    });
    expect(postMessage).toHaveBeenCalledTimes(2);
    expect(postMessage.mock.calls[1][0]).toMatchObject({
      type: "settings-change", sequence: 2, settings: { precision: 5 },
    });

    optimizer.options.precision = 2;
    optimizer.persistSessionState();
    expect(receive({ type: "flush-settings", loadToken: "test-window-token" }, null)).toBeNull();
    expect(receive({ type: "flush-settings", loadToken: "wrong" })).toBeNull();
    expect(postMessage).toHaveBeenCalledTimes(2);
    expect(receive({ type: "flush-settings", loadToken: "test-window-token" })).toEqual({
      type: "flush-settings",
    });
    expect(postMessage.mock.calls[2][0]).toMatchObject({
      type: "settings-change", sequence: 3, flush: true, settings: { precision: 2 },
    });
    expect(Object.keys(postMessage.mock.calls[2][0].settings)).toHaveLength(15);
    expect(JSON.stringify(postMessage.mock.calls[2][0])).not.toContain("svg");
    expect(receive({ type: "settings-result", success: true, sequence: 2, loadToken: "test-window-token" })).toBeNull();
    expect(receive({ type: "settings-result", success: true, sequence: 3, loadToken: "test-window-token" })).toEqual({
      type: "settings-result", success: true,
    });
    expect(postMessage).toHaveBeenCalledTimes(3);
  });

  it("shows host settings failures independently from document saves", async () => {
    vi.useFakeTimers();
    const { App, initializeGlobalHandlers } = await import("../src/ui");
    const { optimizer } = await import("../src/optimizer");
    vi.spyOn(optimizer, "initializeEditor").mockResolvedValue();
    const root = document.createElement("div");
    document.body.append(root);
    try {
      m.render(root, m(App));
      initializeGlobalHandlers();
      optimizer.options.precision = 3;
      optimizer.persistSessionState();
      const send = (data: unknown) => {
        window.dispatchEvent(new MessageEvent("message", { data, source: window }));
        m.render(root, m(App));
      };
      send({ type: "settings-result", success: false, error: "No storage", sequence: 1, loadToken: "wrong" });
      expect(root.querySelector(".settings-error-banner")).toBeNull();
      send({ type: "settings-result", success: false, error: "No storage", sequence: 1, loadToken: "test-window-token" });
      expect(root.querySelector('[role="alert"]')?.textContent).toBe("Failed to save editor settings: No storage");
      send({ type: "load-svg", svg, uri: "file:///image.svg", loadToken: "test-window-token" });
      send({ type: "save-result", success: true, loadToken: "test-window-token" });
      expect(root.querySelector(".settings-error-banner")).not.toBeNull();
      optimizer.persistSessionState();
      expect(postMessage).toHaveBeenCalledTimes(2);
      send({ type: "settings-result", success: true, sequence: 2, loadToken: "test-window-token" });
      expect(root.querySelector(".settings-error-banner")).toBeNull();
    } finally {
      m.render(root, null);
      root.remove();
    }
  });

  it("follows trusted host theme changes without saving a local Procyon theme", async () => {
    vi.useFakeTimers();
    window.procyonPlugin!.theme = "dark";
    localStorage.setItem("svgo-theme", "light");
    const { App, initializeGlobalHandlers } = await import("../src/ui");
    const { optimizer } = await import("../src/optimizer");
    vi.spyOn(optimizer, "initializeEditor").mockResolvedValue();
    const root = document.createElement("div");
    document.body.append(root);
    try {
      m.render(root, m(App));
      initializeGlobalHandlers();
      expect(document.body.classList.contains("theme-light")).toBe(false);
      expect(root.querySelector('[title="Toggle theme"]')).toBeNull();
      const send = (theme: string, loadToken: string, source: MessageEventSource | null = window) =>
        window.dispatchEvent(new MessageEvent("message", {
          data: { type: "theme-change", theme, loadToken }, source,
        }));
      send("light", "wrong");
      send("light", "test-window-token", null);
      expect(document.body.classList.contains("theme-light")).toBe(false);
      send("light", "test-window-token");
      expect(document.body.classList.contains("theme-light")).toBe(true);
      expect(localStorage.getItem("svgo-theme")).toBe("light");
      send("dark", "test-window-token");
      expect(document.body.classList.contains("theme-light")).toBe(false);
    } finally {
      m.render(root, null);
      root.remove();
    }
  });

  it("keeps failed saves visible until retry or a new load", async () => {
    const { App, initializeGlobalHandlers } = await import("../src/ui");
    const { optimizer } = await import("../src/optimizer");
    const editorInit = vi.spyOn(optimizer, "initializeEditor").mockResolvedValue();
    const root = document.createElement("div");
    document.body.append(root);
    vi.useFakeTimers();
    try {
      m.render(root, m(App));
      initializeGlobalHandlers();
      const send = (data: unknown, source: MessageEventSource | null = window) => {
        window.dispatchEvent(new MessageEvent("message", { data, source }));
        m.render(root, m(App));
      };
      send({ type: "load-svg", svg, uri: "file:///image.svg", loadToken: "test-window-token" });
      root.querySelector<HTMLButtonElement>('[title="Save SVG to Procyon"]')?.click();
      m.render(root, m(App));
      expect(root.querySelector(".save-status")?.textContent).toBe("Saving SVG...");

      send({ type: "save-result", success: false, error: "Disk full", loadToken: "wrong" });
      send({ type: "save-result", success: false, error: "Disk full", loadToken: "test-window-token" }, null);
      expect(root.querySelector(".save-status")?.textContent).toBe("Saving SVG...");

      send({ type: "save-result", success: false, error: "Disk full", loadToken: "test-window-token" });
      expect(root.querySelector('[role="alert"]')?.textContent).toBe("Save failed: Disk full");
      vi.advanceTimersByTime(5000);
      expect(root.querySelector('[role="alert"]')?.textContent).toBe("Save failed: Disk full");

      root.querySelector<HTMLButtonElement>('[title="Save SVG to Procyon"]')?.click();
      m.render(root, m(App));
      expect(root.querySelector(".save-status")?.textContent).toBe("Saving SVG...");
      send({ type: "save-result", success: true, loadToken: "test-window-token" });
      expect(root.querySelector(".save-status")?.textContent).toBe("SVG saved.");
      vi.advanceTimersByTime(3000);
      m.render(root, m(App));
      expect(root.querySelector(".save-status")).toBeNull();

      send({ type: "save-result", success: false, loadToken: "test-window-token" });
      expect(root.querySelector('[role="alert"]')?.textContent).toBe("Save failed.");
      send({ type: "load-svg", svg, uri: "file:///second.svg", loadToken: "test-window-token" });
      expect(root.querySelector(".save-status")).toBeNull();
    } finally {
      m.render(root, null);
      vi.useRealTimers();
      editorInit.mockRestore();
    }
  });
});
