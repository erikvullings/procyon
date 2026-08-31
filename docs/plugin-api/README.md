# Plugin API reference

Plugins declare a versioned `plugin.toml`. API version `1` supports only action contributions
(which also supply context-menu and command-palette entries), custom columns, and metadata
extraction. Plugins cannot inject JavaScript or arbitrary WebView UI.

```toml
id = "example.copy-path"
name = "Copy Path"
version = "0.1.0"
api_version = "1"
description = "Copies a selected path"
entrypoint = "plugin.lua"

[permissions]
selected_entry_metadata = true
clipboard_write = true

[contributions]
actions = true
```

Every permission defaults to denied. The explicit keys are `selected_entry_metadata`,
`selected_entry_content_read`, `filesystem_read` (root list), `filesystem_write` (root list),
`clipboard_read`, `clipboard_write`, `network` (host allow-list), `process_spawn`,
`notifications`, and `settings_storage`. Unknown keys and unsupported `api_version` values reject
the manifest. Discovery leaves invalid manifests disabled and returns their diagnostic through the
plugin listing rather than preventing startup.

The initial runtime is restricted Lua. Wasmtime plus the WebAssembly Component Model remains the
distributable target; no native Rust dynamic-library ABI is exposed. See ADR
[0006](../decisions/0006-plugin-runtime-selection.md).

## Lua entrypoint contract and isolation

An entrypoint returns a Lua table. When `contributions.actions = true`, its `actions` field must be
a function returning an array of `{ id, title, description }` action tables. An action table may
also set `requires_single_selection = true` to advertise that it only makes sense when exactly one
entry is selected; the host derives the action's context requirements from this flag and
re-validates them server-side before invoking the action, so the command palette and context menu
disable/hide the action automatically when the requirement is not met. Enabled contributions are
automatically exposed through the shared action registry, so the command palette and context
menus receive them through their normal registry refresh.

### Invoking actions: the `invoke` contract

When an action fires, the host calls the entrypoint's `invoke(action_id)` function with the
action's id as its sole argument. Two host calls are available while `invoke` runs, both
permission-gated:

- `host.selected_entry_metadata()` returns the caller-supplied selection as an array of
  `{ name, uri }` tables (requires the `selected_entry_metadata` permission). The caller already
  knows the current selection's name and file URI (from pane state), so this is the data it passed
  in when invoking the action — the host does not resolve an opaque entry id back to metadata.
- `host.clipboard_write(text)` stages `text` for the host to copy to the clipboard (requires the
  `clipboard_write` permission). The actual OS/browser clipboard write is the caller's
  responsibility (the backend cannot write to a browser client's clipboard); the host publishes a
  success notification and returns `text` as `clipboardText` on the action result so the caller
  can perform it. Calling this without the permission fails visibly with a `PermissionDenied`
  error instead of silently no-op'ing.

The sample plugin `plugins/sample-copy-markdown-path/` implements this contract: it declares
`sample.copyMarkdownPath` with `requires_single_selection = true`, then builds a Markdown link
`[name](uri)` from the selection, Markdown-escaping the name and percent-encoding the URI, before
calling `host.clipboard_write`.

Each call creates a fresh Lua state with only table, string, math, and UTF-8 libraries. `io`,
`os`, `package`, `debug`, process launch, filesystem and network APIs are absent. The optional
`host.selected_entry_metadata()` call is explicitly permission-checked. Calls are bounded by a
100 ms timeout, 100,000 instruction budget, and 4 MiB Lua memory limit. Failures are logged under
the plugin id, create a non-blocking warning notification, and cannot crash the host. Three
consecutive failures auto-disable a plugin; enabling it again clears that automatic disablement.
The runtime keeps the newest 100 diagnostics per plugin for the diagnostics view.

When `contributions.columns = true`, the entrypoint's `columns` field must be a
function returning `{ id, title }` declarations. Column declarations are data only;
the host owns rendering and maps the `sample.fileAge` sample to its compact age
formatter and raw modification-timestamp sort key. This uses no per-row filesystem
calls. A failed or timed-out column declaration is omitted from the plugin listing,
so its table cells remain empty and the directory table continues working.

## Icon theme contribution

A directory-entry icon theme runs no code and needs no `entrypoint` — set
`contributions.icon_theme = true` and add a sibling `icon-theme.json`:

```toml
id = "example.icons"
name = "Example Icons"
version = "1.0.0"
api_version = "1"
description = "A directory-entry icon theme"

[contributions]
icon_theme = true
```

```json
{
  "iconDefinitions": {
    "folder": { "iconPath": "icons/folder.svg" },
    "file": { "iconPath": "icons/file.svg" },
    "symlink": { "iconPath": "icons/symlink.svg" },
    "rust": { "iconPath": "icons/rust.svg" }
  },
  "folder": "folder",
  "file": "file",
  "symlink": "symlink",
  "fileExtensions": { "rs": "rust" },
  "fileNames": { "Cargo.toml": "rust" },
  "mimePrefixes": { "image/": "file" }
}
```

- `iconDefinitions` is a map from an arbitrary, theme-local key to an `iconPath`, an SVG asset
  path relative to the plugin directory. `iconPath` must not be absolute and must not contain a
  `..` component — discovery rejects (disables) the whole plugin otherwise, so an icon theme can
  only ever reference its own files.
- `folder`, `file`, and `symlink` set the default icon definition key used for each entry kind.
  `fileExtensions` maps a lowercased, dot-less extension (e.g. `"rs"`, not `".rs"` or `"RS"`) to a
  definition key; `fileNames` maps an exact file name (e.g. `"Cargo.toml"`, matched case-sensitively
  so a theme can distinguish `Cargo.lock` from `cargo.lock`) to one; `mimePrefixes` maps a MIME type
  prefix (e.g. `"image/"`) to one. Every key referenced by any of these six fields must exist in
  `iconDefinitions`, and `iconDefinitions` must not be empty — both reject the manifest otherwise.
- All top-level fields besides `iconDefinitions` are optional; omit whichever kinds/mappings
  your theme doesn't customize; the built-in default is used for anything left unset.
- Resolution precedence in the frontend (`resolveEntryIcon`, `entry-icons.ts`): directories and
  symlinks always use `folder`/`symlink`. Files try `fileNames` first, then `fileExtensions`, then
  the first `mimePrefixes` entry whose prefix matches the entry's MIME type (insertion order), then
  `file`.
- Icon assets must be SVG. They are fetched over the plugin icon-theme asset endpoint (HTTP route
  and Tauri command, both path-contained to the plugin's own directory) and sanitized in the
  frontend (`svg-sanitizer.ts`) before rendering — only `<svg>`, `<path>`, `<g>`, `<circle>`,
  `<rect>`, `<polygon>` elements and a small allow-list of presentation attributes survive;
  `<script>`, `<foreignObject>`, `on*` handlers, and `href`/`xlink:href` are stripped regardless
  of nesting depth. Keep icons to that element set — anything else is silently removed, not an
  error.
- An icon theme is listed in the Settings editor's "Directory icon theme" picker as soon as its
  plugin is discovered (valid `plugin.toml` + `icon-theme.json`), even before the plugin is
  enabled — labeled "(plugin disabled)" in that case. Selecting it only takes visual effect once
  the plugin is enabled; the asset-serving endpoint refuses to serve any icon for a disabled
  plugin, so the directory table falls back to the built-in generic icons until then.

See `plugins/catppuccin-icons/` for a complete real-world example (28 icon definitions, extension
and MIME mappings, vendored SVGs). For the fuller design rationale (security model, discovery,
serving), see [`docs/architecture/theming.md`](../architecture/theming.md#distributable-icon-theme-plugins-task-0095).
