# Theming

The frontend has one visual token system in `frontend/src/themes/theme.css`. Components consume
`--fm-*` variables and must not declare literal theme colours. A Vitest source guard scans
non-generated frontend TypeScript and CSS and rejects hex colour literals outside `src/themes/`.

## Runtime themes

The root `data-theme` attribute is the runtime interface:

- `data-theme="light"` selects the light palette.
- `data-theme="dark"` selects the dark palette.
- no `data-theme` attribute means follow system. A `prefers-color-scheme: dark` query supplies the
  dark palette; the light palette is the default.

`mithril-materialized`'s `ThemeManager` owns that attribute. Its `light`, `dark`, and `auto` values
therefore switch both Materialized controls and custom file-manager UI without a reload.
Materialized's `--mm-*` variables map back to the corresponding `--fm-*` tokens, including
backgrounds, surfaces, text, borders, inputs, selection, hover, accent, and error colours.
Only mithril-materialized's `core.css`, `forms.css`, `components.css`, and `utilities.css` modules
are imported. They cover the application's form controls, modals, buttons, theme switcher, and
toasts without loading the unused picker and advanced-component groups.
`frontend/src/themes/mithril-materialized-procyon.css` loads afterwards as the application-owned
density and shape layer for mm components in use.

## Token contract

The palette tokens are `--fm-background`, `--fm-surface`, `--fm-surface-elevated`, `--fm-text`,
`--fm-text-muted`, `--fm-border`, `--fm-accent`, `--fm-selection`,
`--fm-selection-inactive`, `--fm-hover`, `--fm-error`, `--fm-warning`, and `--fm-success`.
Density and shape use `--fm-row-height`, `--fm-font-family`, `--fm-font-size`, `--fm-radius`, and
`--fm-shadow`.

Use `--fm-selection` only in the active pane. The shared row convention uses `.fm-selected-row`
for selection and `.fm-pane[data-active="true"]` to promote it from the inactive to active
selection token. Selection has an inset edge marker; `.fm-cursor-row` uses a dashed outline.
Consequently selection and keyboard cursor remain distinguishable without relying on colour.

## Accessibility verification

WCAG AA requires at least 4.5:1 contrast for normal text. The theme test calculates relative
luminance from the shipped token values and enforces that threshold for text on the normal surface,
active selection, and inactive selection.

| Theme | Surface | Active selection | Inactive selection |
| --- | ---: | ---: | ---: |
| Light | 16.27:1 | 12.24:1 | 13.20:1 |
| Dark | 14.40:1 | 7.57:1 | 9.64:1 |

When `prefers-reduced-motion: reduce` is active, the theme stylesheet reduces transitions and
animations to effectively zero duration and disables smooth scrolling.

## Directory entry icons

Per-entry glyphs in the directory table (`frontend/src/features/directory-table/entry-icons.ts`)
are resolved from `entryIconRegistry`, a mutable registry exported from that module rather than
hard-coded in `directory-table.ts`. It holds three maps:

- `kindIcons`: keyed by `EntryKind` (`directory`/`symlink`/`file`), used before any extension/MIME
  match and as the final fallback.
- `extensionIcons`: keyed by lowercased file extension without the leading dot (`png`, `zip`, `pdf`,
  ...), consulted first for `file` entries.
- `mimePrefixIcons`: keyed by a MIME type prefix (`image/`, `audio/`, ...), consulted when an
  entry's extension has no registered icon.

A theme or plugin package overrides or extends the built-in set by mutating these maps directly at
startup, for example:

```ts
import { entryIconRegistry } from '../features/directory-table/entry-icons';
import { psdIcon } from './my-theme-icons';

entryIconRegistry.extensionIcons.set('psd', psdIcon);
```

`createDefaultEntryIconRegistry()` returns a fresh, independent registry (used by tests) built from
the same defaults as the shared `entryIconRegistry` singleton. Every icon renderer has the shape
`(attrs?: IconAttrs) => m.Children`, matching the plain SVG helpers in
`frontend/src/components/icons.ts` (`.fm-icon` class, `currentColor` fill, consistent `viewBox`).
This is a themeable rendering layer only; native OS icons served from the backend
(`runtimeCapabilities.nativeFileIcons`) are a separate, not-yet-implemented overlay tracked by a
follow-up task.

### Distributable icon theme plugins (task 0095)

Icon themes are installed at runtime from **plugin packages**, not compiled into the frontend
bundle. Any plugin directory under `plugins/` (or the user plugin directory) may declare
`[contributions] icon_theme = true` in its `plugin.toml`; unlike action/column plugins, an
icon-theme-only plugin needs no `entrypoint` — there is no code to execute, only a manifest and a
set of SVG assets.

#### Manifest schema

A theme plugin ships a sibling `icon-theme.json` next to `plugin.toml`, modeled on VS Code's file
icon theme format but trimmed to what `entryIconRegistry` resolves:

```json
{
  "iconDefinitions": {
    "folder": { "iconPath": "icons/folder.svg" },
    "file": { "iconPath": "icons/file.svg" },
    "typescript": { "iconPath": "icons/typescript.svg" }
  },
  "folder": "folder",
  "file": "file",
  "symlink": "symlink",
  "fileExtensions": { "ts": "typescript" },
  "fileNames": { "Cargo.toml": "cargo" },
  "mimePrefixes": { "image/": "image" }
}
```

- `iconDefinitions`: maps an arbitrary definition key to an `iconPath` relative to the plugin
  directory. The path must resolve inside that directory — `..` segments and absolute paths are
  rejected.
- `folder`, `file`, `symlink`: definition keys used as the default glyph for each `EntryKind`.
- `fileExtensions`: lowercased extension (no leading dot) to definition key, feeding
  `entryIconRegistry.extensionIcons`.
- `fileNames`: exact, case-sensitive file name to definition key, feeding
  `entryIconRegistry.fileNameIcons` and matched ahead of `fileExtensions`.
- `mimePrefixes`: MIME-type prefix to definition key, feeding `entryIconRegistry.mimePrefixIcons`
  (an fm-specific fallback tier; not part of VS Code's format).

`discover_plugins` parses and validates `icon-theme.json` whenever `icon_theme` is declared, using
the same "invalid plugin → disabled with a diagnostic, never fail startup" pattern as action/column
plugins: an empty `iconDefinitions`, an unsafe `iconPath`, or any `file`/`folder`/`symlink`/
`fileExtensions`/`fileNames`/`mimePrefixes` value referencing an undeclared definition key disables
the plugin with a diagnostic instead of aborting discovery.

#### Serving theme assets

The theme's JSON and each referenced SVG are served read-only, strictly contained to the plugin's
own directory — an `HTTP GET /api/v1/plugins/{pluginId}/icon-theme/asset?path=...` route
(`fm-server`) and an equivalent Tauri `get_plugin_icon_theme_asset` command, both backed by
`Service::plugin_icon_theme_asset`, which rejects any `path` that is not one of the theme's
declared icon paths (including path-traversal attempts), **and requires the plugin to be
currently enabled** — a disabled plugin's assets 404 even if the path is otherwise valid.
`PluginDescriptorDto` carries the plugin's icon theme (id + definitions) so the frontend can list
it without a second discovery mechanism; unlike asset serving, listing is gated only on the
plugin being validly discovered (`is_valid()`), not on enablement, so a theme can be previewed/
selected in Settings before its plugin is enabled — the Settings Editor labels it "(plugin
disabled)" in that case, and the directory table falls back to the built-in generic icons until
the plugin is actually enabled.

#### Security: sanitizing third-party SVGs

Icon markup now comes from a third-party plugin directory rather than a vendored, build-time
constant, so it cannot be handed to `m.trust()` as-is — that would be an XSS vector. Before install,
`frontend/src/themes/svg-sanitizer.ts`'s `sanitizeSvgMarkup()` parses each SVG and rebuilds it from
an allow-list: only `svg`/`path`/`g`/`circle`/`rect`/`polygon` elements and their presentation
attributes (`fill`, `stroke`, `stroke-width`, `d`, `viewBox`, etc.) survive; `<script>`,
`<foreignObject>`, and any `on*` event-handler attribute are stripped unconditionally. This runs
once per icon at theme-install time, not per render.

#### Installing a theme

`frontend/src/themes/plugin-icon-theme.ts`'s `installPluginIconTheme(client, pluginId, iconTheme)`
is the generic, data-driven replacement for the old hardcoded `installCatppuccinIconTheme()`: it
fetches each declared icon asset through `FileManagerClient.getPluginIconThemeAsset()`, sanitizes
it, and installs renderers into `entryIconRegistry` keyed the same way `fileExtensions`/
`mimePrefixes` describe. `restoreDefaultIconTheme()` (in
`frontend/src/features/directory-table/entry-icons.ts`) removes the theme-only entries and
restores the built-in generic set, unchanged from 0085/0092's original restore contract.

`Settings.iconTheme` is an open plugin-id string rather than a closed enum; `'generic'` is the
reserved built-in default that requires no plugin lookup. The Settings Editor's icon-theme
`Select` is populated from `plugins.filter(p => p.iconTheme !== undefined)` — any discovered
icon-theme plugin appears automatically, with no core-repo change required to add a theme.

#### Worked example: the Catppuccin theme package

`plugins/catppuccin-icons/` is a real, distributable plugin package built entirely with this
mechanism — a worked example for third-party theme authors:

```bash
plugins/catppuccin-icons/
├── plugin.toml       # id = "catppuccin.icons", contributions.icon_theme = true, no entrypoint
├── icon-theme.json   # iconDefinitions + file/folder/symlink + fileExtensions + mimePrefixes
└── icons/*.svg       # one SVG per definition key, Catppuccin Mocha palette baked into each
                       # icon's own stroke/fill attributes (no CSS var indirection)
```

Each SVG is a standalone `<svg viewBox="0 0 16 16">...</svg>` document vendored verbatim from the
MIT-licensed [`catppuccin/vscode-icons`](https://github.com/catppuccin/vscode-icons) project (see
`plugin.toml` for the full attribution). Adding or updating a theme is purely a matter of shipping
this kind of package — no PR into this repository, no TypeScript, no rebuild.
