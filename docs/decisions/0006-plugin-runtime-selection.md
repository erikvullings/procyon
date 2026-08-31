# 0006 Plugin runtime selection

Status: accepted

## Context
The file manager must support third-party plugins (e.g. custom columns, context-menu actions)
without letting plugin code access the raw filesystem or the host process directly, since plugins
are less trusted than first-party code (spec §22 plugin model, §35 "must not expose arbitrary
filesystem methods... directly to JavaScript/plugins").

## Decision
`fm-plugin-api` defines a narrow, versioned ABI (the operations and data a plugin may use), and
`fm-plugin-runtime` hosts the first proof of concept in a restricted Lua runtime behind that ABI
rather than granting plugins a native dynamic-library surface or an unrestricted scripting
environment. Wasmtime plus the WebAssembly Component Model is the distributable migration target.
Sample plugins
(`plugins/sample-copy-markdown-path`, `plugins/sample-file-age-column`) are built only against the
published `fm-plugin-api` surface, so the ABI's real usability is exercised by first-party
examples before third parties depend on it.

## Alternatives
- **Native dynamic libraries (`.so`/`.dylib`/`.dll`) loaded directly**: rejected — spec §35
  explicitly forbids exposing native dynamic libraries as the plugin ABI; also unsafe across
  platforms and impossible to sandbox.
- **Full scripting language with unrestricted host bindings** (e.g. arbitrary Lua/JS with
  filesystem access): rejected — same rationale, violates the "no arbitrary filesystem methods to
  plugins" rule and removes any capability boundary.
- **No plugin system, only first-party features**: rejected — extensibility (custom columns,
  actions) is a stated goal of the spec.

## Consequences
- New plugin capabilities require an explicit ABI addition in `fm-plugin-api`, reviewed as a
  capability grant rather than "whatever the host process can do."
- The runtime crate becomes the enforcement point for sandboxing; it must be trusted code even
  though the plugins it hosts are not.
- Sample plugins double as the runtime's acceptance tests: if a sample plugin cannot be expressed
  against the ABI, the ABI is incomplete.

## Revisit conditions
Revisit if the initial ABI proves too narrow for real third-party plugin ideas (e.g. plugins that
need background work or persistent state), or if the chosen runtime's sandboxing/performance
characteristics don't hold up once plugins run in the desktop (Tauri) host.
