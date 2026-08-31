# 0010 Native platform adapters

Status: accepted

## Context
Native desktop integration (native menus, open-with-default-application, reveal in
Finder/Explorer, open terminal, drag-and-drop, system Trash/Recycle Bin, packaging) differs per
operating system and is only available at all when running under Tauri, not in a plain browser
(spec §32 native platform integration; §3 rule 10 explicit capabilities).

## Decision
`fm-platform` defines a cross-platform trait for native integrations, with `fm-platform-macos` and
`fm-platform-windows` providing OS-specific implementations, each compiled only for its target OS
(cfg-gated workspace members, per task 0001). Capabilities not available on a given host (e.g. no
native platform integration at all in the plain-browser transport) are represented explicitly
through the capabilities model rather than assumed to exist (rule 10), so the frontend can hide or
disable unsupported actions instead of invoking them and failing.

## Alternatives
- **A single cross-platform crate with runtime `#[cfg]` branches inline**: rejected — mixes
  platform-specific `unsafe`-adjacent native API calls into one crate, making it harder to keep
  `unsafe_code = "deny"` at the workspace level (task 0001) for the platform-neutral code.
  Per-OS crates isolate that surface.
  A Linux crate is deliberately not added yet — no Linux-native integration is scoped in the current
  spec; add `fm-platform-linux` if and when that's required rather than stubbing it speculatively
  (spec §35).
- **Skip native integration in the browser build entirely, always assume Tauri**: rejected — the
  browser host is a first-class target (ADR 0001); native-integration actions must be capability-
  gated, not compiled-in assumptions.
- **Shell out to OS-specific CLI tools instead of native APIs**: rejected as the default — slower,
  harder to sandbox/error-handle than native platform APIs, and less portable across OS versions.

## Consequences
- Each native platform crate is untestable on other operating systems by construction; CI (task
  0004) must run the Rust job matrix across macOS and Windows runners for this code to be exercised
  at all, and any behaviour only reachable on Linux stays genuinely untested until such a crate
  exists.
- The frontend must query capabilities before offering platform-specific actions (e.g. "Reveal in
  Finder" only shown when that capability is present), rather than always rendering every action.
- Packaging (installers, code signing where applicable) is scoped per platform crate/app, not
  centralised, since packaging mechanics differ fundamentally between macOS and Windows.

## Revisit conditions
Revisit if Linux native integration becomes a requirement (add `fm-platform-linux` following the
same pattern), or if enough behaviour turns out to be shareable across macOS/Windows that the
per-OS split creates more duplication than isolation benefit.
