# SVGO panel

The packaged `dist/` is built from the `svgo-procyon-save` branch of
`erikvullings/svgo` at commit `d11f373c09eab688f186a1a620dfe2fedf13dc54`
with `pnpm build:procyon`. It includes local Monaco resources; no network or
general filesystem capability is granted.

Enable **SVGO** in plugin settings, position the active pane's cursor on an SVG
file, and use **Cmd+Shift+F4** on macOS (**Ctrl+Shift+F4** elsewhere) or the
command palette. The editor opens as a tab in the opposite pane, titled
**SVGO: filename**. Switch to other tabs without losing unsaved changes; the
live editor tab cannot be dragged to another pane. Close it with its tab-strip
close button or **Cmd+W** (Ctrl+W elsewhere).
It starts in Tree view with a closed menu and an even vertical stack: the tree
occupies the top half and the SVG preview the bottom half. Save is the primary
button, and Copy is in the menu; File → Open and the in-app title are absent
in the Procyon build.
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
