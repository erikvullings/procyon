# 0226 Sandboxed JavaScript and SPA plugins

Status: open
Priority: medium
Subsystem: cross-cutting
Depends on: 0053, 0054, 0057

## Context

Procyon's versioned plugin API currently supports restricted Lua action/column code and declarative
icon themes. It intentionally excludes arbitrary JavaScript and WebView UI. Users want richer
integrations such as an SVGO tool or other self-contained SPAs without weakening the existing
capability boundary.

Tauri makes rendering packaged HTML straightforward, but a child WebView is not a security boundary
by itself: untrusted JavaScript must not inherit Procyon's Tauri command surface, filesystem/process
authority, credentials, or unrestricted network access. This task adds JavaScript and SPA support
behind the same explicit, versioned plugin permissions as Lua rather than creating an unrestricted
second plugin system.

## Acceptance Criteria

- The manifest distinguishes execution runtime (`lua` or sandboxed `javascript`) from contribution
  type (actions/columns versus an SPA panel); existing API-v1 Lua plugins remain compatible.
- JavaScript action plugins run without Node.js, direct Tauri APIs, DOM access, ambient filesystem,
  process, credential, or network authority. Host services are deny-by-default and exposed only
  through typed, permission-checked plugin API calls.
- JavaScript execution has enforced time/instruction, memory, output-size, and cancellation limits,
  with the same diagnostics, failure isolation, repeated-failure auto-disable, and re-enable behavior
  as Lua plugins.
- SPA assets load only from the installed plugin package in an isolated child WebView with a unique
  plugin identity/origin, strict CSP, blocked arbitrary navigation and popups, and no direct access
  to Procyon's Tauri invoke surface.
- SPA-to-host communication uses a narrow authenticated message bridge. Every request binds the
  calling WebView to its plugin ID, validates a versioned request schema, checks the declared
  capability, applies input/output limits, and returns typed errors.
- Network, selected-file content, filesystem writes, clipboard access, settings storage, and process
  execution remain separate reviewed capabilities. The SPA cannot bypass them with browser APIs.
- Closing, disabling, uninstalling, crashing, or timing out a plugin tears down its WebView/runtime,
  pending requests, temporary data, and subscriptions without affecting the main app.
- A sample JavaScript plugin and a sample packaged SPA exercise discovery, enable/disable, action or
  panel activation, bounded host messaging, and failure handling. Include an SVGO-shaped transform
  example if it can run without adding Node authority.
- Browser-host behavior is explicit and parity-safe: either use an equivalently isolated supported
  implementation or report the SPA capability unavailable rather than silently changing security
  semantics.
- Tests cover manifest compatibility, runtime selection, resource budgets, capability denial,
  plugin-ID spoofing, Tauri-command denial, CSP/navigation restrictions, malformed/oversized
  messages, lifecycle cleanup, and macOS/Windows/Linux desktop smoke paths.
- A focused security review and threat model are completed before marking the task done.

## Implementation Notes

- Keep one `fm-plugin-api` contract and one plugin manager. Runtime language and UI contribution are
  orthogonal; do not fork permissions, discovery, diagnostics, or management state into Lua and SPA
  variants.
- Evaluate an embedded non-Node engine such as QuickJS or Boa for JavaScript actions. Record the
  choice in an ADR because deterministic interruption, memory accounting, maintenance, and platform
  packaging matter more than Node compatibility.
- Build SPA panels with dedicated Tauri WebViews/windows whose labels and origins are allocated by
  the trusted host. Do not expose the main application's generated Tauri bindings to plugin assets.
- Tauri capability configuration alone may not be sufficient for dynamically installed plugin IDs.
  The trusted bridge must authorize the concrete WebView label/origin and plugin ID on every call.
- Reuse the existing plugin failure budgets and diagnostics in
  `crates/fm-plugin-runtime/src/lib.rs`, manifest/permission types in
  `crates/fm-plugin-api/src/lib.rs`, and management UI from task 0057.
- Coordinate with 0142: sanitized plugin preview output remains distinct from an interactive SPA
  panel, and previewed files must never become executable content.
- Do not use a Node sidecar or unrestricted shell/process access as the JavaScript runtime.

## Agent Notes

- 2026-09-18 Copilot: Created from the proposal to support both Lua plugins and JavaScript SPAs in
  the Tauri host. The central constraint is that rendering a WebView is easy but securely binding
  dynamic third-party code to Procyon's capability model is the actual feature. No implementation
  has started.
- 2026-10-03 Copilot: Implemented the API-v1 Lua/JavaScript runtime selection, bounded QuickJS
  actions, declarative SPA manifests, isolated Tauri scheme/bridge prototype, cursor-only panel
  action, a JavaScript sample, and a packaged SVGO editor with explicit revision-checked Save.
  `Cmd+F4` already sorts by extension, so SVGO uses `Cmd+Shift+F4`. The focused review and
  [threat model](../docs/plugin-api/spa-threat-model.md) leave this task **open**: WKWebView
  cannot deny clipboard reads on macOS, and the user declined that grant, so macOS panels fail
  closed; Windows/Linux native smoke paths are unverified. Immediate WebView-crash cleanup,
  cancellation-safe temporary-file cleanup, and atomic cross-process save conflicts are still
  release gates. At that point bundled SVGO was unavailable on macOS.
- 2026-10-03 Copilot: The user authorized clipboard read as well as write for SVGO. Its manifest
  now declares both, and macOS panel activation is allowed only for plugins with both grants;
  other panels remain unavailable. The task remains open pending native platform smoke tests,
  a fresh macOS security review, and the other release gates in the threat model.
- 2026-10-03 Copilot: The bundled editor now occupies the opposite pane in its own native child
  WebView rather than a separate window. Placement and close commands are restricted to the
  owning trusted window. The source pane restores its cursor by file URI after atomic Save
  replaces an entry ID, unless the user has moved the cursor. Native platform smoke tests and
  the other release gates remain outstanding.
- 2026-10-03 Copilot: The editor now owns a transient, switchable opposite-pane tab. Tab
  switches hide but retain its child WebView and unsaved work; closing the tab tears it down.
  Its Procyon-specific build starts in Tree view with a closed menu, the tree above
  the preview in equal-height halves, and puts
  Save in the primary button and Copy in the menu, and retains optimizer settings but
  never the SVG or separate theme. Native interaction and remaining security release gates
  are still unverified. Since private child WebViews cannot guarantee localStorage persistence,
  optimizer preferences use a validated, permission-checked host settings bridge; normal tab
  close waits for a bounded final flush. Abrupt teardown can still lose the latest preference.
- 2026-10-03 Copilot: The Procyon-only SVGO build now uses Procyon's raised surface for the
  toolbar and preview work area, removes the permanent SVG canvas border, and softens the
  divider without reducing its drag target. It also removes the Source SVG row, moves Tree/Code
  into the menu, starts with Properties collapsed in a side rail, and relayouts Monaco when
  opening Code. Native WKWebView Code rendering still needs verification.
- 2026-10-03 Copilot: Reduced the embedded Code gutter, removed its focus border, and restored
  small Code/Tree insets. A WebKit timing repro exposed a moving editor during the sidebar
  transition and a zero-size Monaco startup while Tree was visible; the Procyon build now
  disables that transition and defers Monaco creation until Code is shown. WebKit resize,
  selection, and typing checks pass, but the reported persistent native WKWebView caret and
  glyph-painting fault still requires confirmation in the Tauri child WebView.
- 2026-10-03 Copilot: At the user's request, the Procyon-specific SVGO panel now offers
  Tree editing and SVG preview only; Code and all packaged Monaco assets are removed.
  Standalone SVGO retains Code. Tree edits still update the preview and require an explicit
  Save. The native Monaco failure is bypassed rather than diagnosed or fixed.
- 2026-10-03 Copilot: Mouse-wheel zoom in the SVGO preview now anchors the SVG point beneath
  the pointer. It compensates after layout changes such as scrollbars appearing or
  disappearing, while toolbar zoom and Reset retain their previous behavior.
- 2026-10-03 Copilot: The Procyon preview hides zoom-induced scrollbars without disabling
  drag panning. Its viewport background remains fixed as artwork zooms and pans, with
  a presentation-only Light, Dark, or Checkerboard selector; Light and Dark use the
  matching Procyon status-bar colors. The default follows the current theme unless
  explicitly overridden, and Save does not include the background.
- 2026-10-03 Copilot: Procyon's SVGO panel source, tests, static assets, locked
  dependencies, and build configuration now live in `plugin-sources/svgo/`. Its
  reproducible `build:procyon` output is copied into the checked-in
  `plugins/svgo/dist/` package. The source stays outside Tauri's bundled
  `plugins/` resource directory to avoid shipping build dependencies.
- 2026-10-03 Copilot: Release security review found that Tauri's window-scoped native
  plugin permissions also applied to child SPA WebViews, bypassing the app-command
  label guard. The desktop capability is now scoped to trusted WebView labels
  instead. Native denial and platform smoke tests remain release gates.
  A feature-gated Tauri smoke harness now exercises a real child WebView's bundled SVGO load,
  updater-command denial and bridge Save on CI's three desktop platforms; the macOS developer
  run passed, but Windows/Linux CI and the remaining native/release gates are outstanding.
- 2026-10-04 Copilot: The file editor now tracks sibling temporary copies through Save and
  attempts cleanup on write/commit errors and dropped save futures; it cancels the provider
  operation before discarding with a fresh token. This narrows the timed-out Save leak, but
  process crashes, runtime shutdown, provider deletion/upload races, and ambiguous commits
  remain. Atomic cross-process revision checks, immediate child-WebView crash cleanup, and
  native platform smoke/review gates are still outstanding; the task remains open.
- 2026-10-04 Copilot: PR #87's native SPA CI smoke timed out on all three hosts because its
  unbundled release build retained Tauri's Vite `devUrl` without `custom-protocol`. The smoke
  feature now enables embedded release assets; a no-Vite macOS release run passed twice through
  real child UI, updater ACL denial, and revision-checked Save. Stage diagnostics and daily log
  capture replace the opaque timeout. Windows/Linux release smoke results and the other release
  gates remain pending; the task stays open.
- 2026-10-04 Copilot: Extended the feature-gated native smoke through a real installed package.
  A smoke-identity DMG was mounted, copied to a canonical macOS temporary path, and passed
  bundled SVGO asset discovery, child UI, updater ACL denial, and revision-checked Save without
  using repository plugins. Canonicalizing the install path matters because Tauri rejects the
  `/var` symlink ancestor on macOS. CI is configured to perform the same check with MSI and DEB
  packages; those platforms, the normal package's cursor/shortcut path, and the remaining
  security gates still require qualification. The task remains open.
