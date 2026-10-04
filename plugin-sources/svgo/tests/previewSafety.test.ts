// @vitest-environment jsdom
import m from "mithril";
import { JSDOM, requestInterceptor } from "jsdom";
import { afterEach, describe, expect, it } from "vitest";
import { PreviewPanel } from "../src/components/previewPanel";
import { SVGOptimizer } from "../src/optimizer";

const maliciousSvg = `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" xml:base="http://127.0.0.1:65432/" onload="window.previewExecuted=true">
  <script>window.previewExecuted=true</script>
  <foreignObject><img src="http://127.0.0.1:65432/foreign" onerror="window.previewExecuted=true"></foreignObject>
  <image href="http://127.0.0.1:65432/image"/>
  <image href="#relative-image"/>
  <feImage href="http://127.0.0.1:65432/filter-image"/>
  <use xlink:href="http://127.0.0.1:65432/symbol#icon"/>
  <use href="javascript:window.previewExecuted=true"/>
  <image href="data:image/svg+xml;base64,PHN2ZyBvbmxvYWQ9YWxlcnQoMSk+"/>
  <style>@import url("http://127.0.0.1:65432/style"); .shape{fill:url("http://127.0.0.1:65432/paint")}</style>
  <style>@\\69mport url("http://127.0.0.1:65432/escaped")</style>
  <rect class="shape" width="10" height="10" style="stroke:url(http://127.0.0.1:65432/stroke)" filter="url(http://127.0.0.1:65432/filter)"/>
  <rect width="4" height="4" style="fill:image-set('http://127.0.0.1:65432/image-set')"/>
  <rect width="3" height="3" style="stroke:u\\72l(http://127.0.0.1:65432/escaped-url)"/>
  <defs><linearGradient id="local"><stop stop-color="red"/></linearGradient></defs>
  <circle r="5" fill="url(#local)"/>
</svg>`;

describe.each(["standalone", "Procyon"] as const)("%s SVG preview", (mode) => {
  afterEach(() => {
    delete window.procyonPlugin;
    document.body.replaceChildren();
  });

  it("renders safe artwork without executable content or external resources", async () => {
    if (mode === "Procyon") {
      window.procyonPlugin = { loadToken: "test", postMessage: () => {} };
    }
    const root = document.createElement("div");
    document.body.append(root);
    m.render(root, m(PreviewPanel, {
      previewSvg: maliciousSvg,
      theme: "dark",
      splitOrientation: "vertical",
      onToggleSplitOrientation: () => {},
      onZoomIn: () => {},
      onZoomOut: () => {},
      onResetZoom: () => {},
    }));
    const preview = root.querySelector(".preview-container")!;
    expect(preview.querySelector("svg rect")).not.toBeNull();
    expect(preview.querySelector("svg circle[fill='url(#local)']")).not.toBeNull();
    expect(preview.querySelector("script, foreignObject, style")).toBeNull();
    expect(preview.innerHTML).not.toMatch(/127\.0\.0\.1|javascript:|data:image\/svg\+xml|onload=|onerror=/i);

    const requests: string[] = [];
    const isolated = new JSDOM(`<!doctype html><body>${preview.innerHTML}</body>`, {
      url: "http://127.0.0.1:65433/",
      runScripts: "dangerously",
      resources: {
        interceptors: [requestInterceptor(async (request) => {
          requests.push(request.url);
          return new Response("");
        })],
      },
    });
    isolated.window.document.querySelector("svg")?.dispatchEvent(new isolated.window.Event("load"));
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(isolated.window.eval("window.previewExecuted")).toBeUndefined();
    expect(requests).toEqual([]);
    isolated.window.close();
  });

  it("keeps safe inline styles, stylesheet colors, local references, and embedded raster images", () => {
    const svg = `<svg xmlns="http://www.w3.org/2000/svg">
      <style>.shape{fill: red; stroke: url(#local)}</style>
      <defs><linearGradient id="local"><stop stop-color="blue"/></linearGradient><circle id="shape" r="3"/></defs>
      <use href="#shape" class="shape"/>
      <rect width="5" style="fill:blue;opacity:.5" fill="url(#local)"/>
      <image href="data:image/png;base64,iVBORw0KGgo="/>
    </svg>`;
    const root = document.createElement("div");
    document.body.append(root);
    m.render(root, m(PreviewPanel, {
      previewSvg: svg,
      theme: "dark",
      splitOrientation: "vertical",
      onToggleSplitOrientation: () => {},
      onZoomIn: () => {},
      onZoomOut: () => {},
      onResetZoom: () => {},
    }));
    const preview = root.querySelector(".preview-container")!;
    expect(preview.querySelector("style")?.textContent).toContain("fill: red");
    expect(preview.querySelector("use")?.getAttribute("href")).toBe("#shape");
    expect(preview.querySelector("rect[width='5']")?.getAttribute("style")).toContain("fill:blue");
    expect(preview.querySelector("rect[width='5']")?.getAttribute("fill")).toBe("url(#local)");
    expect(preview.querySelector("image")?.getAttribute("href")).toMatch(/^data:image\/png/);
  });

  it("sanitizes the temporary live DOM used for SVG bounds measurement", () => {
    let measuredMarkup = "";
    Object.defineProperty(SVGElement.prototype, "getBBox", {
      configurable: true,
      value: function (this: SVGElement) {
        measuredMarkup = this.outerHTML;
        return { x: 0, y: 0, width: 10, height: 10 };
      },
    });
    try {
      new SVGOptimizer().calculateContentBBox(maliciousSvg);
      expect(measuredMarkup).toContain('fill="url(#local)"');
      expect(measuredMarkup).not.toMatch(/127\.0\.0\.1|<script|foreignObject|onload=/i);
    } finally {
      Reflect.deleteProperty(SVGElement.prototype, "getBBox");
    }
  });

  it("leaves the editable SVG source unchanged", () => {
    const optimizer = new SVGOptimizer();
    optimizer.loadSvgString(maliciousSvg);
    expect(optimizer.getSourceSvg()).toContain("onload=");
    expect(optimizer.getSourceSvg()).toContain("foreignObject");
  });
});
