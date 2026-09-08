# 0201 Structured knowledge architecture and Zvec audit

Status: done
Priority: high
Subsystem: architecture, search, semantic
Depends on: 0181, 0182, 0183

## Context

The restored Structured Knowledge Query specification replaces the mistaken assumption behind
0199/0200. Procyon's primary knowledge feature must retrieve useful source material without an LLM,
remote provider, or network access. Audit the existing semantic worker, Zvec 0.7.0 bindings,
index/catalog migration machinery, host boundaries, and search UI before fixing the implementation
contract for tasks 0202-0208.

## Acceptance Criteria

- Document the existing Zvec collection schema, content/vector fields, index metadata, update and
  deletion behavior, query path, and migration/rebuild mechanisms.
- Verify the exact pinned Rust API for native FTS, runtime index creation, MultiQuery/RRF, schema
  inspection, and multilingual tokenization on every supported semantic target.
- Define host-neutral capability, request, plan, evidence, trace, and optional-answer boundaries
  that preserve the semantic worker's lack of filesystem and LLM authority.
- Define a non-destructive vector-only-to-FTS migration and FTS-only fallback when query embedding
  is unavailable or incompatible.
- Record the sequential task contracts and remove the misleading LLM query-planning control from
  Ask without deleting benchmark evidence.

## Implementation Notes

- Source of truth: the 2026-09-08 restored “Procyon Structured Knowledge Query” discussion.
- Keep `KnowledgeSearchRequest` separate from optional `KnowledgeAnswerRequest`.
- Do not add another search engine when Zvec native FTS can satisfy the contract.
- Do not let `do:` or `to:` contaminate retrieval unless context is explicitly repeated in
  `related:`.

## Agent Notes

- 2026-09-08 Copilot: Started after the requester corrected the LLM rewrite interpretation.
  Researching the exact zvec-rust 0.7.0 FTS and migration APIs while removing the developer Ask
  opt-in that exposed the wrong experiment.
- 2026-09-08 Copilot: Audited the pinned Rust SDK and the worker's schema, writes, deletes, dense
  query, manifest, and authoritative SQLite rebuild path. Recorded the exact FTS, runtime-index,
  schema-inspection, MultiQuery, and RRF APIs in `docs/architecture/zvec-rust-sdk.md`; accepted ADR
  0012 for the host-neutral request/plan/evidence/trace/optional-answer boundaries and staged
  schema-v2 migration. Removed the misleading Ask query-planning control and its orphaned
  localization while retaining backend benchmark evidence. Verified the seven focused Ask tests,
  frontend typecheck, Impeccable detector, and repository whitespace checks.
