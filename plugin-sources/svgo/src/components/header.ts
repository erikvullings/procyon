import m from "mithril";
import { iconCopy, iconSave } from "./actionIcons";

export type HeaderStats = {
  originalSizeLabel: string;
  optimizedSizeLabel: string;
  reductionLabel: string;
  reductionClass: string;
};

export type HeaderAttrs = {
  stats: HeaderStats;
  showTitle: boolean;
  onToggleSidebar: () => void;
  canOptimize: boolean;
  onOptimize: () => void;
  canCopy: boolean;
  isCopied: boolean;
  onCopy: () => void;
  onSave?: () => void;
  canSave?: boolean;
};

export const Header: m.Component<HeaderAttrs> = {
  view({ attrs }) {
    const {
      stats,
      showTitle,
      onToggleSidebar,
      canOptimize,
      onOptimize,
      canCopy,
      isCopied,
      onCopy,
      onSave,
      canSave,
    } = attrs;
    return m(".header", [
      m(".header-left", [
        m(
          "button.menu-toggle",
          { onclick: onToggleSidebar, title: "Toggle sidebar" },
          iconMenu(),
        ),
        showTitle
          ? m(".title", [
              m("img.logo", { src: "logo.svg", alt: "Logo" }),
              m("span", "Advanced SVG Optimizer"),
            ])
          : null,
      ]),
      m(".stats", [
        m(".header-actions", [
          m(
            "button.view-toggle.active.header-optimize-btn",
            {
              onclick: onOptimize,
              disabled: canOptimize ? undefined : true,
              title: "Apply optimized SVG to source",
            },
            "Optimize",
          ),
          onSave
            ? m(
                "button.copy-btn.header-copy-btn",
                {
                  onclick: onSave,
                  disabled: canSave ? undefined : true,
                  title: "Save SVG to Procyon",
                },
                [iconSave(), m("span", "Save")],
              )
            : m(
                "button.copy-btn.header-copy-btn",
                {
                  onclick: onCopy,
                  disabled: canCopy ? undefined : true,
                  title: "Copy source SVG to clipboard",
                },
                [
                  isCopied
                    ? m(
                        "svg[width=16][height=16][viewBox=0 0 24 24][fill=none][stroke=currentColor][stroke-width=2][stroke-linecap=round][stroke-linejoin=round]",
                        [
                          m(
                            "path[stroke-linecap=round][stroke-linejoin=round][d=M20 6L9 17l-5-5]",
                          ),
                        ],
                      )
                    : iconCopy(),
                  m("span", isCopied ? "Copied!" : "Copy"),
                ],
              ),
        ]),
        m(".stat", [
          m(".stat-label", "Original"),
          m(".stat-value", stats.originalSizeLabel),
        ]),
        m(".stat", [
          m(".stat-label", "Optimized"),
          m(".stat-value", stats.optimizedSizeLabel),
        ]),
        m(".stat", [
          m(".stat-label", "Reduction"),
          m(
            ".stat-value",
            { class: stats.reductionClass },
            stats.reductionLabel,
          ),
        ]),
      ]),
    ]);
  },
};

function iconMenu(): m.Vnode {
  return m(
    "svg.icon[viewBox=0 0 24 24][fill=none][stroke=currentColor][stroke-width=2]",
    [m("path[d=M3 6h18]"), m("path[d=M3 12h18]"), m("path[d=M3 18h18]")],
  );
}
