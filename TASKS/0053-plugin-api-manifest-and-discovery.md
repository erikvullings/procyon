# 0053 Plugin API, manifest, discovery and permissions

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: plugins
Depends on: 0049

## Context
`file-manager-coding-agent-spec.md` §19 and §33 step 9. The plugin API must be suitable for later
Wasm isolation and must never expose unstable Rust ABI types (§19.4, §35).

## Acceptance Criteria
- `fm-plugin-api` defines the versioned, stable plugin-facing contract: action contributions,
  column contributions, metadata extraction, and the host services plugins may call.
- Manifest parsing per §19.1 (`id`, `name`, `version`, `api_version`, `description`, `entrypoint`,
  `[permissions]`, `[contributions]`), with a versioned schema and clear validation errors.
- Discovery scans a plugin directory, validates manifests, and reports invalid plugins as disabled
  with a diagnostic rather than failing startup.
- Permission model per §19.3 covering: selected-entry metadata, selected-entry content read,
  filesystem read scopes, filesystem write scopes, clipboard read, clipboard write, network host
  allow-list, process execution, notifications, settings storage.
- Nothing is granted by default (§19.3); every host call checks its permission and returns a typed
  denial that is logged and surfaced.
- Contribution types limited to §19.2: actions, context-menu and palette entries via actions, custom
  columns, metadata extraction. Plugins cannot inject JavaScript or arbitrary UI (§19.2).
- `GET /api/v1/plugins`, `POST /api/v1/plugins/{pluginId}/enable`, `.../disable`
  (`listPlugins`, `enablePlugin`, `disablePlugin`), mirrored as Tauri commands; enabled state
  persists in settings.
- Unit tests: manifest validation (including unknown `api_version` and unknown permission keys),
  permission enforcement, discovery of a malformed plugin.
- `docs/plugin-api/` documents the manifest and the contribution/permission model.

## Implementation Notes
- Record the runtime choice in ADR 6 (task 0005): restricted Lua for the first proof of concept,
  Wasmtime + Component Model as the distributable target (§19.4).
- No public native Rust dynamic-library ABI (§35).

## Agent Notes
- 2026-08-01 codex: Added the versioned `fm-plugin-api` manifest contract, explicit default-deny
  permissions, contribution declarations, typed permission denials, and a narrow host-services
  interface without a native Rust ABI. `fm-plugin-runtime` discovers direct-child plugin manifests
  deterministically and retains malformed ones as disabled diagnostics. `FileManagerService`
  projects discovery to REST/Tauri and persists enablement in `Settings`; added
  `GET /api/v1/plugins`, plus enable/disable endpoints and matching Tauri commands. Generated the
  OpenAPI/Orval client, documented the manifest and permission model, and recorded restricted Lua
  with Wasmtime Component Model migration in ADR 0006. Verified 6 task-specific tests (4 API,
  1 malformed-discovery, 1 REST) plus affected Rust package tests, `cargo fmt --all --check`, and
  frontend `tsc --noEmit`.
