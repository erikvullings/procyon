# SVGO panel

The checked-in `dist/` is a baseline; desktop packaging regenerates it from the
[source and build instructions](../../plugin-sources/svgo/README.md) in this
repository, initially copied from `erikvullings/svgo` at commit
`ee29ff2c46912dc9318fbaf8f5d408e898a71f7b`. The Procyon build is
Tree-only and omits Monaco; no network or general filesystem capability is
granted. Do not hand-edit the generated `dist/` files.

Enable **SVGO** in plugin settings, position the active pane's cursor on an SVG
file, and use **Cmd+Shift+F4** on macOS (**Ctrl+Shift+F4** elsewhere) or the
command palette. The editor opens as a tab in the opposite pane, titled
**SVGO: filename**. Switch to other tabs without losing unsaved changes; the
live editor tab cannot be dragged to another pane. Close it with its tab-strip
close button or **Cmd+W** (Ctrl+W elsewhere).
It starts in Tree view with a closed menu and an even vertical stack: the tree
occupies the top half and the SVG preview the bottom half. The Properties panel
starts collapsed and can be expanded from the side rail. Save is the primary
button, and Copy is in the menu; File → Open, the Source SVG row, and the
in-app title are absent in the Procyon build. The toolbar and preview work
area match Procyon's status-bar surface. The borderless SVG preview has a fixed
background selectable as Light (the light status-bar color), Dark (the dark
status-bar color), or Checkerboard. Its default follows the active theme,
including live theme changes until a background is explicitly selected. The
background does not zoom or pan with the artwork or become part of the saved
SVG. The split handle keeps its full draggable area. Mouse-wheel zoom keeps
the SVG point beneath the pointer stationary. Preview scrollbars stay hidden,
while dragging can still pan zoomed artwork; the toolbar zoom controls retain
their existing behavior.
Tree has a small vertical inset. The standalone SVGO app retains its Code view.
Save writes only the originally opened file and rejects revisions that changed
outside the panel. A successful atomic Save keeps the source pane's cursor on
the SVG when its filesystem entry identity changes. Download remains available
when running SVGO standalone. Procyon retains optimizer preferences in its
plugin settings across isolated WebView sessions; tab close requests a final
snapshot and logs a failure if it cannot be persisted. It retains neither
the SVG content nor its file URI, view mode, sidebar state, or split
layout. The theme follows Procyon's light/dark setting and palette; the plugin
does not store a separate theme.

The plugin declares clipboard read and write permissions. WKWebView exposes
both capabilities directly to page JavaScript on macOS, so they cannot be
confined to the Save bridge. See the
[threat model](../../docs/plugin-api/spa-threat-model.md) for remaining release gates.
