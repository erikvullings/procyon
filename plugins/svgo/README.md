# SVGO panel

The packaged `dist/` is built from the `svgo-procyon-save` branch of
`erikvullings/svgo` at commit `501f8d1` with `pnpm build:procyon`. It includes
local Monaco resources; no network or general filesystem capability is granted.

Enable **SVGO** in plugin settings, position the active pane's cursor on an SVG
file, and use **Cmd+Shift+F4** on macOS (**Ctrl+Shift+F4** elsewhere) or the command palette.
Save writes only the originally opened file and rejects revisions that changed
outside the panel. Download remains available when running SVGO standalone.
