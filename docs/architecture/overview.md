# Architecture overview

This document is prose, not a source of truth: the crate layering it describes is enforced
mechanically by [`crates/fm-test-support/src/architecture.rs`](../../crates/fm-test-support/src/architecture.rs)
and checked against the real `cargo metadata` graph in
`crates/fm-test-support/tests/workspace_architecture.rs`. If this document and that code ever
disagree, the code is correct — fix the prose, not the enforcement.

## Layering diagram (spec §3)

```text
┌───────────────────────────────────────────────────────────┐
│ Shared Mithril frontend                                   │
│                                                           │
│ Components, state, workspaces, pane views, dialogs         │
│                                                           │
│ Depends only on FileManagerClient                         │
└───────────────────────────┬───────────────────────────────┘
                            │
             ┌──────────────┴───────────────┐
             │                              │
┌────────────▼─────────────┐   ┌────────────▼──────────────┐
│ HTTP client adapter      │   │ Tauri client adapter      │
│                          │   │                           │
│ Generated REST client    │   │ Tauri invoke commands     │
│ EventSource/SSE           │   │ Tauri channels/events     │
└────────────┬─────────────┘   └────────────┬──────────────┘
             │                              │
             └──────────────┬───────────────┘
                            │
┌───────────────────────────▼───────────────────────────────┐
│ Rust application services                                │
│                                                           │
│ Navigation, workspaces, actions, operations, search,      │
│ metadata, plugins, settings and event publication         │
└───────────────────────────┬───────────────────────────────┘
                            │
┌───────────────────────────▼───────────────────────────────┐
│ Rust domain and engine                                    │
│                                                           │
│ VFS providers, operation scheduler, conflict resolution,  │
│ directory snapshots, filesystem watching and journaling   │
└───────────────────────────────────────────────────────────┘
```

The crate-level version of this same direction (domain → events / vfs traits / plugin API →
operations / providers / metadata / search → application services → Axum and Tauri hosts) is the
`CRATE_LAYERS` table in `architecture.rs` linked above; see that file for the authoritative,
per-crate layer assignment.

## Mandatory rules (spec §3)

These ten rules govern every change to the frontend/backend boundary and the crate graph. They are
restated verbatim here so they can be found without opening the full specification; the
enforceable subset of them (layering, and the Axum/Tauri/anyhow dependency bans) is also asserted
by `architecture.rs`.

1. Frontend components must not call `fetch`, `EventSource` or Tauri APIs directly.
2. Axum handlers must remain thin.
3. Tauri commands must remain thin.
4. Core engine crates must not depend on Axum or Tauri.
5. Transport DTOs must not be reused indiscriminately as internal domain models.
6. Long-running operations must be represented as jobs.
7. The backend must own authoritative filesystem and operation state.
8. The frontend may hold presentation state, but must not implement file-copy semantics.
9. Browser and Tauri transports must provide equivalent application behaviour.
10. Platform differences must be represented through explicit capabilities.

Rules 1, 6, 8 and 9 are about where behaviour lives (frontend vs. backend, browser vs. Tauri) and
are reviewed by hand, since they concern intent rather than a dependency graph. Rules 2, 3 and 5 are
about handler/command thinness and DTO reuse, also reviewed by hand at the point handlers and
commands are added. Rule 4 and the `thiserror`/`anyhow` split from spec §2.2 are the two rules
`architecture.rs` checks mechanically today, via `HOST_ONLY_DEPENDENCIES` and
`APPLICATION_BOUNDARY_DEPENDENCIES`.

## Why this split

The dual-host requirement (browser + Tauri, spec §3 and ADR
[0001](../decisions/0001-browser-tauri-dual-host-architecture.md)) means the frontend cannot own any
transport-specific code: everything above the `FileManagerClient` boundary must work unmodified
against either adapter. Pushing operation state, filesystem truth and long-running work into the
Rust services (rules 6–9) is what keeps that boundary honest instead of becoming a leaky
abstraction that only browser or only Tauri actually satisfies.

## Related documents

- `docs/decisions/` — one ADR per architectural decision in spec §34.
- `docs/plugin-api/` — plugin API reference (task 0005 adds only a placeholder; filled in when the
  plugin runtime work lands).
- `docs/screenshots/` — UI screenshots referenced from the README and docs (placeholder for now).
