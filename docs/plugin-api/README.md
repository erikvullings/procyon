# Plugin API reference

Plugins declare a versioned `plugin.toml`. API version `1` supports action contributions
(which also supply context-menu and command-palette entries), custom columns, metadata
extraction, icon themes, and isolated SPA panels. Runtime language and contribution type are
independent: existing manifests default to Lua; `runtime = "javascript"` explicitly selects
the embedded sandboxed JavaScript executor for actions/columns. The SPA's JavaScript runs in
an isolated WebView, not in the action executor.

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
`selected_entry_content_read`, `selected_entry_content_write`, `filesystem_read` (root list), `filesystem_write` (root list),
`clipboard_read`, `clipboard_write`, `network` (host allow-list), `process_spawn`,
`notifications`, and `settings_storage`. Unknown keys and unsupported `api_version` values reject
the manifest. Discovery leaves invalid manifests disabled and returns their diagnostic through the
plugin listing rather than preventing startup.
`selected_entry_content_write` grants only a reviewed, selected-entry-specific host write call;
it does not grant `filesystem_write` roots or authority to write arbitrary paths. The host must
bind a write to the opened selection and validate its revision independently of the plugin.

The default runtime is restricted Lua. Wasmtime plus the WebAssembly Component Model remains the
distributable target; no native Rust dynamic-library ABI is exposed. See ADR
[0006](../decisions/0006-plugin-runtime-selection.md).

## JavaScript action and SPA panel manifest

```toml
id = "example.svg-tool"
name = "SVG Tool"
version = "1.0.0"
api_version = "1"
description = "An SVG action and optional panel"
runtime = "javascript"
entrypoint = "actions.js"

[permissions]
selected_entry_metadata = true
clipboard_write = true

[contributions]
actions = true

[contributions.spa_panel]
entrypoint = "dist/index.html"
action_id = "example.svg-tool.open"
title = "Optimize SVG"
shortcut = "Cmd+Shift+F4"
extensions = ["svg"]
```

The action/column `entrypoint` is independent of `contributions.spa_panel.entrypoint`. A
panel-only plugin can omit the action entrypoint. `action_id` must be namespaced by the plugin
id (e.g. `example.svg-tool.open`); `title` is the host action label, and optional `shortcut`
is a bounded ASCII key-combination hint whose actual binding the host controls. Opening the
panel is declarative and does not require a second JavaScript action declaration. The panel
entrypoint must be a package-relative `.html` path without traversal; extensions are optional
lowercase, dotless, unique ASCII alphanumeric file extensions. An empty list allows any
file extension. `Cmd+F4` is already used by Sort by Extension, so the bundled SVGO panel
uses `Cmd+Shift+F4` (`Ctrl+Shift+F4` outside macOS).
Panel assets must stay inside the installed package, including after symlink resolution.
Declaring a panel does **not** grant filesystem, network, Tauri, clipboard, or host-process
authority; an enabled panel requires a separately isolated host implementation and a
permission-checked message bridge. A host unable to provide that boundary must report the panel
unavailable, not render it in the main app.

JavaScript entrypoints evaluate to an object (for example
`({ actions() { return [...] }, invoke(actionId) { ... } })`) with `actions()` and/or `columns()`
returning the same data shapes as Lua. `invoke(actionId)` may call
`host.selected_entry_metadata()` and `host.clipboard_write(text)` only with the corresponding
permissions. There is no Node.js, DOM, Tauri, filesystem, process, credentials, or ambient
network API in the embedded executor. Each call runs in a fresh bounded context with a 100 ms
deadline, 4 MiB VM memory cap, instruction interrupt budget, 1 MiB source cap, and 256 KiB
serialized-output/clipboard cap. Clipboard requests are staged for the caller rather than
written directly by the backend. Callers can pass a `PluginCancellation` to interrupt
long-running calls; cancelled calls retain the same diagnostics and failure accounting as
other failures.

`plugins/sample-js-svg-uri/` is a bundled action example: it uses only the selected-entry
metadata and clipboard-write calls. `plugins/svgo/` bundles a separate, panel-only editor,
which requires selected-entry content read/write, clipboard read/write, and plugin-scoped
settings storage, but no general filesystem or network grant.
Both are disabled until enabled in Settings. The SVG panel opens the file under the **cursor**
(not a marked selection) as a transient tab in the opposite pane, in a separate child WebView
rather than in the trusted application's WebView. Switching tabs hides the child without
discarding its in-memory edits; closing its tab (including Cmd+W on macOS), disabling the
plugin, or closing the host releases that WebView. Only a bounded, explicitly validated
optimizer-settings snapshot persists through Procyon's settings store across private
WebView sessions. Closing a tab requests a final snapshot and waits briefly for it before
teardown; a failed or timed-out flush is logged, not treated as success. The source SVG is
not restored, and Procyon's theme controls the panel.
Save writes the original file with revision checking and preserves the source cursor
across an atomic file replacement. In the
browser/server host, the panel action is explicitly unavailable. On macOS, WKWebView exposes
clipboard read and write directly to panel JavaScript; only panels declaring both grants can
open. See the [threat model and remaining release gates](spa-threat-model.md).

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
