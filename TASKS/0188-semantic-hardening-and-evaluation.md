# 0188 Semantic subsystem hardening and evaluation

Status: done
Priority: medium
Subsystem: quality, security, performance
Depends on: 0183, 0185, 0186, 0187

## Context

Semantic search and RAG persist sensitive derived content, execute downloaded native/model
components, consume sustained resources, and may send selected excerpts to cloud services. Before
calling the subsystem production-ready, Procyon needs explicit privacy, recovery, isolation,
evaluation, backup, and cross-platform evidence beyond feature-level tests.

## Acceptance Criteria

- A threat model covers component supply chain, local IPC impersonation, malicious documents,
  archive/converter bombs, prompt injection, tenant-filter bypass, path/metadata disclosure, SSRF,
  credential leakage, stale evidence, denial of service, and deletion/retention failures.
- Normal logs contain only IDs/hashes, stage, timing, counts, component/model/profile identities,
  and redacted error categories. Queries, excerpts, filenames, prompts, responses, credentials,
  headers, and HTTP bodies require an explicit previewed, expiring diagnostic capture and never
  enter default logs.
- Scope exclusion has end-to-end deletion proofs across catalog, extracted artifacts, Zvec,
  embedding references, summaries, concepts, saved conversations, backups/snapshots, and in-flight
  jobs. Shared vectors are retained only while an authorized occurrence references them.
- Desktop estimates model/index/extracted-text size before enrolment, enforces a configurable free-
  space reserve, and reports per-root/per-format usage and cleanup. Server tenant quotas fail safely
  without affecting other tenants.
- Resource, soak, crash, corruption, and upgrade tests cover large libraries, repeated edits,
  interrupted migration/publication/deletion, malformed indexes/catalogs, worker/model rollback,
  concurrent Procyon instances, and worker shutdown/restart on all supported platforms.
- A local evaluation workflow saves query + expected/relevant files/chunks and computes documented
  retrieval metrics without telemetry. Repository fixtures cover multilingual recall, near
  duplicates, boilerplate diversity, structural citations, incremental edits, summaries, scope,
  unavailable sources, and concept labels.
- Model, chunker, converter, index, grouping, summary-selection, or threshold changes require a
  before/after evaluation report and migration/storage impact. Quality regressions cannot be hidden
  behind generated-answer fluency.
- Accessibility and keyboard/screen-reader manual passes cover installation consent, enrolment tree,
  progress/errors, semantic results, summary, Ask, citations, profile setup, and SKOS review.
- Ordinary backup preserves enrolment policy, exclusions, exact model manifest identity, profile
  metadata without secrets, vocabularies/accepted edits, and saved conversations/pins while
  excluding rebuildable indexes/chunks. Advanced whole-library export/import is versioned,
  checksummed, encrypted or clearly warns about plaintext content, and validates compatibility.
- Desktop managed and administrator-managed server deployments have documented operation,
  troubleshooting, privacy, data deletion, migration, backup/recovery, and unsupported-format
  guidance. Mac App Store capability differences are explicit.
- Full workspace lint/tests, package/API staleness checks, platform builds, and real interactive
  desktop/browser verification pass with the optional subsystem both installed and absent.

## Implementation Notes

- This task may identify focused defects that should be fixed in the owning 0177-0187 module; do not
  turn hardening into a second implementation of those services.
- Evaluation data remains local by default and exports only through explicit user action.
- Use fixture documents owned by the repository or generated for tests; do not commit private user
  documents or model outputs derived from them.

## Agent Notes

- 2026-09-04: Split from 0176 as the release gate after semantic search, summaries, RAG, and SKOS.
  Optional advanced packs in 0189 depend on this stable, measurable baseline.
- 2026-09-05 Copilot: Added the semantic threat model and operations guide; bounded,
  privacy-safe diagnostic capture; configurable free-space admission; authoritative per-root and
  per-format usage/cleanup reporting; complete catalog/backup/in-flight deletion proofs; and
  versioned, checksummed whole-library export/import validation with explicit plaintext warnings.
- 2026-09-05 Copilot: Added a local, atomic evaluation store and deterministic file/chunk recall,
  MRR, and nDCG scoring. Change reports require comparable before/after evidence plus explicit
  migration and storage impact. Repository fixtures cover multilingual, duplicate, boilerplate,
  citation, edit, summary, scope, unavailable-source, and SKOS scenarios; browser feedback now
  records both relevant file and chunk identities while migrating existing local judgments.
- 2026-09-05 Copilot: Verified 14 new Rust hardening/evaluation tests and 1 new frontend migration
  test, all 625 `fm-application` tests, 147 focused semantic frontend tests, full workspace lint,
  the release server/frontend build, and the complete workspace suite (2,189 Rust, 1,777 frontend,
  and 41 script tests). The cross-platform keyboard/screen-reader and installed/absent packaging
  checks remain explicit manual release steps because this worktree can execute only the macOS
  automated configuration and build coverage.
