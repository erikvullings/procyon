import m from "mithril";
import { SVGOptimizer } from "../optimizer";
import { Controls } from "./controls";
import { iconCopy } from "./actionIcons";

export type SidebarAttrs = {
  optimizer: SVGOptimizer;
  sourceSvg: string;
  theme: "dark" | "light" | "auto";
  onToggleTheme?: () => void;
  open: boolean;
  showFileActions: boolean;
  showDownload: boolean;
  onCopy?: () => void;
  isCopied?: boolean;
};

export const Sidebar: m.Component<SidebarAttrs> = {
  view({ attrs }) {
    const {
      optimizer,
      sourceSvg,
      theme,
      onToggleTheme,
      open,
      showFileActions,
      showDownload,
      onCopy,
      isCopied,
    } = attrs;

    const hasSource = sourceSvg && sourceSvg.trim().length > 0;

    return m(
      "aside.sidebar",
      { class: open ? "open" : "collapsed" },
      m(".sidebar-inner", [
        m(".sidebar-section", [
          m(".section-title", "Actions"),
          showFileActions
            ? m(".file-input", [
                m("input[type=file]", {
                  id: "file-input",
                  accept: ".svg,image/svg+xml",
                  onchange: (e: Event) => {
                    const input = e.target as HTMLInputElement;
                    if (input.files && input.files[0]) {
                      optimizer.loadFile(input.files[0]);
                    }
                  },
                }),
              ])
            : null,
          m(".action-grid", [
            showFileActions
              ? m(
                  "label.action-button.file-button",
                  { for: "file-input", title: "Open SVG file" },
                  [iconFolder(), m("span", "Open")],
                )
              : null,
            showDownload
              ? m(
                  "button.action-button",
                  {
                    title: "Download optimized SVG",
                    onclick: () => optimizer.downloadSvg(),
                    disabled: hasSource ? undefined : "disabled",
                  },
                  [iconDownload(), m("span", "Download")],
                )
              : null,
            onCopy
              ? m(
                  "button.action-button",
                  {
                    title: "Copy source SVG to clipboard",
                    onclick: onCopy,
                    disabled: hasSource ? undefined : "disabled",
                  },
                  [iconCopy(), m("span", isCopied ? "Copied!" : "Copy")],
                )
              : null,
            m(
              "button.action-button",
              {
                title: "Optimize",
                disabled: hasSource ? undefined : "disabled",
                onclick: () => optimizer.loadOptimizedFile(),
              },
              [iconSpark(), m("span", "Optimize")],
            ),
            m(
              "button.action-button",
              {
                title: "Autocrop viewBox",
                disabled: hasSource ? undefined : "disabled",
                onclick: () => optimizer.autocropCurrentSvg(),
              },
              [iconCrop(), m("span", "Autocrop")],
            ),
            m(
              "button.action-button",
              {
                title: "Remove all groups",
                disabled: hasSource ? undefined : "disabled",
                onclick: () => optimizer.removeGroupsCurrentSvg(),
              },
              [iconUngroup(), m("span", "Remove groups")],
            ),
            m(
              "button.action-button",
              {
                title: "Undo",
                disabled: !optimizer.canUndo(),
                onclick: () => {
                  optimizer.undo();
                  m.redraw();
                },
              },
              [iconUndo(), m("span", "Undo")],
            ),
            m(
              "button.action-button",
              {
                title: "Redo",
                disabled: !optimizer.canRedo(),
                onclick: () => {
                  optimizer.redo();
                  m.redraw();
                },
              },
              [iconRedo(), m("span", "Redo")],
            ),
          ]),
        ]),
        m(".sidebar-section.options", [m(Controls, { optimizer })]),
        onToggleTheme
          ? m(".sidebar-footer", [
              m(".section-title", "Theme"),
              m(
                "button.action-button",
                { onclick: onToggleTheme, title: "Toggle theme" },
                [
                  iconTheme(),
                  m(
                    "span",
                    theme === "dark"
                      ? "Light"
                      : theme === "light"
                        ? "Auto"
                        : "Dark",
                  ),
                ],
              ),
            ])
          : null,
      ]),
    );
  },
};

function iconFolder(): m.Vnode {
  return m(
    "svg.icon[viewBox=0 0 24 24][fill=none][stroke=currentColor][stroke-width=1.8]",
    [
      m("path[d=M3 7h5l2 2h11v9a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z]"),
      m("path[d=M3 7V5a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v2]"),
    ],
  );
}

function iconDownload(): m.Vnode {
  return m(
    "svg.icon[viewBox=0 0 24 24][fill=none][stroke=currentColor][stroke-width=1.8]",
    [
      m("path[d=M12 3v10]"),
      m("path[d=M8 9l4 4 4-4]"),
      m("path[d=M4 17h16v4H4z]"),
    ],
  );
}

function iconSpark(): m.Vnode {
  return m(
    "svg.icon[viewBox=0 0 24 24][fill=none][stroke=currentColor][stroke-width=1.8]",
    [
      m("path[d=M12 2l1.8 4.6L18 8l-4.2 1.4L12 14l-1.8-4.6L6 8l4.2-1.4z]"),
      m("path[d=M5 18l.8 2L8 21l-2.2.9L5 24l-.8-2L2 21l2.2-.9z]"),
    ],
  );
}

function iconCrop(): m.Vnode {
  return m(
    "svg.icon[viewBox=0 0 24 24][fill=none][stroke=currentColor][stroke-width=1.8]",
    [
      m("path[d=M7 3v12a2 2 0 0 0 2 2h12]"),
      m("path[d=M3 7h12a2 2 0 0 1 2 2v12]"),
    ],
  );
}

function iconUngroup(): m.Vnode {
  return m(
    "svg.icon[viewBox=0 0 24 24][fill=none][stroke=currentColor][stroke-width=1.8]",
    [
      m("rect[x=3][y=6][width=7][height=6][rx=1]"),
      m("rect[x=14][y=12][width=7][height=6][rx=1]"),
      m("path[d=M11 9h2]"),
      m("path[d=M12 10v2]")
    ],
  );
}

function iconUndo(): m.Vnode {
  return m(
    "svg.icon[viewBox=0 0 24 24][fill=none][stroke=currentColor][stroke-width=1.8]",
    [m("path[d=M9 7H4v5]"), m("path[d=M4 12c1.6-3.5 5-6 9-6a9 9 0 0 1 9 9]")],
  );
}

function iconRedo(): m.Vnode {
  return m(
    "svg.icon[viewBox=0 0 24 24][fill=none][stroke=currentColor][stroke-width=1.8]",
    [
      m("path[d=M15 7h5v5]"),
      m("path[d=M20 12c-1.6-3.5-5-6-9-6a9 9 0 0 0-9 9]"),
    ],
  );
}

function iconTheme(): m.Vnode {
  return m(
    "svg.icon[viewBox=0 0 24 24][fill=none][stroke=currentColor][stroke-width=1.8]",
    [
      m("circle[cx=12][cy=12][r=4]"),
      m("path[d=M12 2v2]"),
      m("path[d=M12 20v2]"),
      m("path[d=M4.9 4.9l1.4 1.4]"),
      m("path[d=M17.7 17.7l1.4 1.4]"),
      m("path[d=M2 12h2]"),
      m("path[d=M20 12h2]"),
      m("path[d=M4.9 19.1l1.4-1.4]"),
      m("path[d=M17.7 6.3l1.4-1.4]"),
    ],
  );
}
