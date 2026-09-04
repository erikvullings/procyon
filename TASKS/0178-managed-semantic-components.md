# 0178 Managed semantic components and model packs

Status: open
Priority: high
Subsystem: desktop, packaging, settings
Depends on: 0058, 0177

## Context

The semantic worker, embedding runtime, and models can be a large download and must not ship or
install merely because Procyon is installed. Users need explicit lifecycle actions and enough size,
license, compatibility, and privacy information to make an informed choice.

Desktop Procyon manages these optional components. Server deployments remain administrator
provisioned and browser clients must not gain authority to download executables or enrol server
paths.

## Acceptance Criteria

- Settings expose distinct Install/Enable, Pause indexing, Remove index, Move data, and Uninstall
  components actions. A single ambiguous enable/disable toggle is not used.
- First installation displays signed worker/runtime/model components, versions, licenses, download
  sizes, estimated installed size and RAM, local-only embedding disclosure, semantic-data location,
  and minimum free-space reserve before consent.
- Platform-specific worker/runtime bundles and curated model manifests are signed and checksummed.
  Installation is atomic, resumable, rejects incompatible or tampered content, and rolls back to the
  last working worker after a failed update.
- The normal model catalog records an immutable upstream revision, license, tokenizer, dimensions,
  embedding normalization, runtime compatibility, language coverage, and disk/RAM estimates.
  Arbitrary URLs are never fetched automatically.
- Expert local-model import validates all required metadata and creates a distinct explicit model
  migration. It cannot silently replace the active embedding space.
- Setup recommends a compact multilingual profile and also explains compact English and
  multilingual quality profiles. Persist an abstract profile plus the exact resolved model
  revision used by the index.
- Compatible worker patch releases may update automatically. Embedding-model or schema-affecting
  changes always require confirmation, estimated work, and a resumable full reindex.
- A configurable semantic-data root defaults to platform app data and contains clearly separated
  catalog, extracted content, Zvec, embedding cache, models, and worker versions. Moving it performs
  pause-copy-verify-switch with rollback rather than editing a path in place.
- UI reports component status and per-category disk use. Uninstalling components requires an
  explicit decision about deleting remaining indexes; removing enrolment-derived data cannot be
  bypassed by saved conversations.
- `fm-server` reports administrator-provisioned component capabilities but rejects browser-managed
  installation or arbitrary path changes. Mock mode simulates each lifecycle state.
- Tests cover no-download defaults, consent, checksum/signature rejection, interrupted downloads,
  rollback, data-root migration, low-disk handling, model migration, uninstall, HTTP/Tauri
  capability parity, and server authority checks.

## Implementation Notes

- Store no credentials in ordinary settings. Component manifests are data, not executable plugin
  manifests, and must remain distinct from the Lua plugin system.
- Keep the direct-distribution and Mac App Store variants in mind: runtime-downloaded executable
  code may be unavailable in a sandboxed App Store build and must be capability-gated rather than
  weakening that build's security model.
- Do not pick a concrete default model without multilingual retrieval, CPU latency, license,
  package-size, and index-size measurements.

## Agent Notes

- 2026-09-04: Split from 0176. Model identity is deliberately immutable for an existing library;
  changing it is a migration owned jointly with 0181/0182, never a settings-only switch.
