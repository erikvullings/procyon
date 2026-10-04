import m from "mithril";
import { procyonPlugin } from "../procyon";
import { sanitizePreviewSvg } from "../svgPreview";

type PreviewBackground = "light" | "dark" | "checkerboard";
let previewBackground: PreviewBackground | null = null;

export type PreviewPanelAttrs = {
  previewSvg: string;
  theme: "light" | "dark";
  splitOrientation: "vertical" | "horizontal";
  onToggleSplitOrientation: () => void;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onResetZoom: () => void;
};

function updatePreviewSvgSizing(container: Element): void {
  const svg = container.querySelector("svg");
  if (!svg) return;

  const hasWidth = svg.hasAttribute("width");
  const hasHeight = svg.hasAttribute("height");

  svg.classList.toggle("preview-svg", true);
  svg.classList.toggle("preview-svg-auto-size", !hasWidth && !hasHeight);
}

export const PreviewPanel: m.Component<PreviewPanelAttrs> = {
  oncreate({ dom }) {
    const container = (dom as Element).querySelector(".preview-container");
    if (container) {
      updatePreviewSvgSizing(container);
    }
  },
  onupdate({ dom }) {
    const container = (dom as Element).querySelector(".preview-container");
    if (container) {
      updatePreviewSvgSizing(container);
    }
  },
  view({ attrs }) {
    const {
      previewSvg,
      theme,
      splitOrientation,
      onToggleSplitOrientation,
      onZoomIn,
      onZoomOut,
      onResetZoom,
    } = attrs;
    const background = previewBackground ?? theme;

    return m(".preview-panel#right-panel", [
      m(".panel-header", [
        m("span", "Optimized SVG"),
        previewSvg &&
          m("div.preview-controls", [
            procyonPlugin
              ? m("label.preview-background-control", [
                  m("span", "Background"),
                  m("select", {
                    value: background,
                    onchange: (event: Event) => {
                      const value = (event.target as HTMLSelectElement).value;
                      if (value === "light" || value === "dark" || value === "checkerboard") {
                        previewBackground = value;
                      }
                    },
                  }, [
                    m("option[value=light]", "Light"),
                    m("option[value=dark]", "Dark"),
                    m("option[value=checkerboard]", "Checkerboard"),
                  ]),
                ])
              : null,
            m(
              "button.preview-control-btn",
              {
                onclick: onToggleSplitOrientation,
                title:
                  splitOrientation === "vertical"
                    ? "Switch to horizontal split"
                    : "Switch to vertical split",
              },
              "Split",
            ),
            m("button.preview-control-btn", { onclick: onZoomIn }, "+"),
            m("button.preview-control-btn", { onclick: onZoomOut }, "−"),
            m("button.preview-control-btn", { onclick: onResetZoom }, "Reset"),
          ]),
      ]),
      m(".preview-container", {
        "data-preview-background": procyonPlugin ? background : undefined,
      }, [
        previewSvg
          ? m.trust(sanitizePreviewSvg(previewSvg))
          : m("div.preview-placeholder", "Preview will appear here"),
      ]),
    ]);
  },
};
