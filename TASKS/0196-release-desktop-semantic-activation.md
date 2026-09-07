# 0196 Release desktop semantic activation

Status: done
Priority: high
Subsystem: frontend, backend, release
Depends on: 0194

## Context

Release desktop builds currently remain inert because they do not inject a trusted
`ManagedSemanticComponentCapability`; the development bundle is rejected outside debug builds.
Wire the production catalog into the desktop host and expose the existing consent, installation,
indexing, search, summary, and Ask flows without changing ordinary file-manager startup.

## Acceptance Criteria

- Release desktop construction injects the managed semantic capability with the production
  verifying key and catalog, while normal startup remains network-free and does not launch a
  worker before semantic use.
- Settings presents exact download, disk, RAM, model, license, and privacy information before
  explicit installation and recursive folder-enrolment consent.
- Installed worker/runtime/model resolution uses only verified managed paths and rejects developer
  bundles, stale artifacts, incompatible versions, and missing native dependencies.
- Semantic features behave identically through supported desktop hosts after installation and show
  actionable unavailable/install/update states before installation.
- Updating worker, runtime, model, converter, chunker, or index compatibility uses the existing
  staged migration/rollback flow without deleting enrolment policy or silently mixing generations.
- Tests cover absent, installing, installed, upgrade-required, rollback, uninstall-retain, and
  uninstall-delete states in both host wiring and frontend presentation.

## Implementation Notes

- Extend `apps/fm-desktop/src-tauri/src/semantic_developer.rs` only where reusable host composition
  belongs there; production configuration must not depend on development paths or keys.
- Reuse `ComponentManager` and `ManagedSemanticComponentCapability`.
- The server remains administrator-provisioned and must not inherit desktop download behavior.

## Agent Notes

- 2026-09-07 Copilot: Created as the user-facing activation step. It may proceed in parallel with
  0195 once 0194 fixes the catalog and trust contract, but cannot be called release-ready until
  real platform payloads exist.
- 2026-09-09 Copilot: Wired release desktop construction to a locally embedded, production-signed
  catalog in `apps/fm-desktop/src-tauri/src/semantic_production.rs` and kept catalog loading
  network-free. Release jobs now wait for the matching signed catalog, bundle only that catalog,
  and compile the protected public verification key; payload downloads remain consent-driven HTTPS
  range requests. Managed worker launch resolves and revalidates the active worker, runtime, and
  model on every launch, including the native runtime copy, so upgrades cannot reuse stale or
  tampered generations. Generalized desktop reconciliation from developer-only to managed
  components, preserved the explicit Intel macOS unavailable case, and extended frontend lifecycle
  coverage for absent, downloading, host download policy, rollback, and uninstall states. Updated
  `docs/semantic-operations.md`; the server remains unchanged and administrator-provisioned.
