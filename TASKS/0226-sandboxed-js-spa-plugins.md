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
