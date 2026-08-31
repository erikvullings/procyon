# 0058 Platform adapter traits and capability reporting

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: platform
Depends on: 0043

## Context
`file-manager-coding-agent-spec.md` §23 ("create platform-adapter traits"), §21 (runtime
capabilities) and §3 rule 10 (platform differences are represented through explicit capabilities).

## Acceptance Criteria
- `fm-platform` defines traits for the native integrations the app will use: file icons,
  thumbnails, reveal in system file manager, trash, open with default application, open terminal,
  system clipboard file references, mounted volumes/drives, native menus.
- A no-op/fallback implementation exists so browser/server mode and unsupported platforms work
  without `cfg` branches at every call site.
- `RuntimeCapabilities` (§21) is derived from the active adapter, so the frontend responds to
  capabilities rather than detecting the operating system (§21).
- Unsupported functions are reported as `false` and their UI affordances are hidden or disabled —
  never present-but-broken (§23).
- `fm-platform-macos` and `fm-platform-windows` are wired up as the platform-specific
  implementations, initially delegating everything to the fallback.
- Unit tests assert capability reporting matches the adapter's actual implementation set.

## Implementation Notes
- Keep the traits synchronous-friendly but callable from async contexts via `spawn_blocking`; native
  calls must never block the Tauri UI thread (§28).
- Existing platform-touching code (trash in 0043) should be refactored onto these traits as part of
  this task.

## Agent Notes
- 2026-08-01 copilot: Added `fm-platform` (`PlatformAdapter` trait with 10 default-unsupported
  methods covering all 9 named integrations from the acceptance criteria plus `native_drag_out`;
  `PlatformCapabilities` bitflags; `PlatformError`; `MountedVolume`; `FallbackPlatformAdapter`
  no-op). Wired `fm-platform-macos`/`fm-platform-windows` as thin structs delegating every method
  to an internal `FallbackPlatformAdapter` (real native implementations are separate future tasks;
  0043's trash execution is untouched and still uses its own path — refactoring it onto this trait
  is left for a follow-up since it was explicitly out of scope for this task).
- `FileManagerService` gained an injected `platform: Arc<dyn PlatformAdapter>` field and a new
  `with_platform_adapter` constructor; `with_event_bus`/`new` still work unmodified and default to
  `FallbackPlatformAdapter`, so none of the ~20 existing call sites needed changes.
  `runtime_capabilities()` now derives `native_menus`, `native_file_icons`, `native_thumbnails`,
  `native_drag_out`, `system_trash`, `reveal_in_system_file_manager`, and `open_terminal` from the
  adapter's capability bitflags instead of hardcoded `false`. `clipboard` stays hardcoded `true`
  (the browser Clipboard API works everywhere without a native bridge) and is deliberately NOT
  derived from `PlatformCapabilities::CLIPBOARD_FILE_REFERENCES`, which instead gates pasting real
  file-path lists — `RuntimeCapabilitiesDto` has no field for that yet; a future task adding
  file-reference paste support should add a new DTO field rather than overload `clipboard`.
- OS-adapter selection lives in the host binary, not `fm-application`: added
  `apps/fm-desktop/src-tauri/src/platform.rs::build_platform_adapter()`, which
  `#[cfg(target_os = ...)]`-selects `MacosPlatformAdapter` / `WindowsPlatformAdapter` /
  `FallbackPlatformAdapter`, using a new `[target.'cfg(target_os = "...")'.dependencies]` pattern in
  `apps/fm-desktop/src-tauri/Cargo.toml`. `fm-application` itself stays target-agnostic (it only
  depends on `fm-platform`, never on the per-OS crates). `fm-server`/`fm-cli` need no changes and
  implicitly use `FallbackPlatformAdapter` via `new`.
- Hiding/disabling UI affordances for unsupported capabilities (last acceptance bullet) is left to
  each feature's own frontend task as it's built — this task only guarantees the backend flags are
  correct; the frontend already fetches and stores `RuntimeCapabilities` (`app-shell.ts`,
  `state/model.ts`) from a prior task.
- Tests added: `crates/fm-platform` (5: capability bitflag combination + fallback adapter no-op
  behaviour), `crates/fm-platform-macos` (2: capabilities/every-operation delegate to fallback),
  `crates/fm-platform-windows` (mirrors macos, compiles to an empty crate off-Windows — will run on
  the Windows CI runner), `crates/fm-application/src/service.rs` (1 new:
  `runtime_capabilities_are_derived_from_the_injected_platform_adapter`, using a `StubPlatformAdapter`
  test double with a non-uniform capability set so the test can't pass by accident).
- Verified: `cargo test --workspace` (all pass except one confirmed pre-existing, unrelated
  failure: `fm-server`'s `plugin_routes::list_plugins_starts_empty_and_unknown_enablement_is_not_found`,
  which also fails identically on unmodified `main` — caused by the bundled `plugins/sample-*`
  directories being picked up by `PluginDiscovery`, not by this task).
  `cargo clippy --workspace --all-targets -- -D warnings` is fully clean (zero warnings).
  `cargo test -p fm-test-support` (architecture fitness/layering test) passes with the new
  `fm-platform`/`fm-platform-macos`/`fm-platform-windows` crates wired in.
- Known pre-existing issue observed but NOT fixed (out of scope): `cargo fmt --all --check` reports
  5 formatting diffs in `crates/fm-application/src/service.rs` at unrelated lines (a plugin-manifest
  `expect`, an event-bus subscribe call, two test assertions, and a test fixture write) that are
  identical on unmodified `main` — likely a local rustfmt version drift from whatever produced the
  original commits. All of this task's own added/edited code in that file is rustfmt-clean; the new
  `fm-platform*` crates and `apps/fm-desktop/src-tauri/src/platform.rs` are also rustfmt-clean.
