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

Task 0226 adds a separately selected, embedded QuickJS executor through `rquickjs` for
JavaScript code contributions. QuickJS is packaged in-process across desktop platforms and
supports memory limits and an interrupt handler for deterministic time/instruction budgets,
without Node.js or the browser DOM. A fresh context exposes only typed, permission-checked host
calls; it has no Tauri command, filesystem, process, credentials, or ambient network binding.
The manifest defaults to Lua for backward compatibility. An independent `spa_panel` contribution
identifies packaged HTML for a separately isolated child WebView; choosing JavaScript as an
action runtime neither requires nor grants a panel. A WebView is not itself a security boundary:
trusted host identity binding, CSP, navigation denial and a narrow capability-checked bridge
must be supplied by the desktop host before panels can be enabled. Browser hosts without an
equivalent isolation boundary report the capability unavailable.

## Alternatives
- **Native dynamic libraries (`.so`/`.dylib`/`.dll`) loaded directly**: rejected — spec §35
  explicitly forbids exposing native dynamic libraries as the plugin ABI; also unsafe across
  platforms and impossible to sandbox.
- **Full scripting language with unrestricted host bindings** (e.g. arbitrary Lua/JS with
  filesystem access): rejected — same rationale, violates the "no arbitrary filesystem methods to
  plugins" rule and removes any capability boundary.
- **Node.js sidecar**: rejected — Node's filesystem, process and network APIs create a larger
  authority and packaging surface than an embedded engine with explicitly installed host calls.
- **Boa for embedded JavaScript**: deferred — it is pure Rust, but QuickJS exposes direct VM
  memory limits and an interrupt callback that can bound even tight loops in the chosen binding.
  Both require an explicit host-call allow-list; neither makes an SPA WebView safe by itself.
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
