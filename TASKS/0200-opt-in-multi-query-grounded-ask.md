# 0200 Opt-in multi-query grounded Ask

Status: done
Priority: medium
Subsystem: frontend, backend, rag
Depends on: 0199

## Context

Task 0186 implemented grounded Ask as a host-owned two-stage flow: one exact user question is
embedded locally, evidence is retrieved under an authorized scope and inspected, and only then is a
cited answer generated. Task 0199 evaluates the conservative alternative assumed from the missing
handoff: a bounded planner rewrites/decomposes the question into a small query set and deterministic
fusion combines their local retrieval results.

Implement this strategy only if 0199 records a **go**. If it records **no-go**, cancel this task or
retain the implementation behind a developer-only experiment; do not expose it as an improvement.
Single-query retrieval remains the default, compatibility baseline, explicit comparison control,
and visible fallback when planning cannot safely complete.

## Acceptance Criteria

- Ask exposes a typed retrieval strategy with `singleQuery` as the backward-compatible default and
  `multiQuery` as an explicit per-conversation opt-in. Existing requests, saved conversations, and
  mock fixtures that omit the field migrate to `singleQuery`; semantic search outside Ask is
  unchanged.
- Multi-query preview makes at most one bounded planning request through the selected
  `LlmProfileService` profile and produces no more than four total unique queries including the
  unchanged user question. It sends no retrieved excerpts, paths, filenames, source IDs,
  credentials, action/tool schemas, or broadenable scope metadata to the planner.
- Local/cloud profile status and the fact that planning sends the question before evidence preview
  are visible before opt-in. Existing normalized-host cloud consent and server allow-list/SSRF
  policy apply to the planning call exactly as they do to answer generation.
- Every planned query clones the same host-resolved tenant/library/root/workspace filters,
  additional authorized tenant IDs, exact source restriction, current hashes, and retrieval policy.
  Planner output is data only and cannot select a scope, alter filters, enrol content, read another
  source, change score constraints, or invoke an action.
- The original question is always retrieved. Each ranked list applies the existing absolute and
  relative score constraints before the 0199-selected deterministic fusion. Results are
  deduplicated by stable evidence/occurrence identity with stable tie-breakers, after which the
  existing per-document diversity, adjacent structural expansion, complete-chunk packing, and
  global context-token budget run once.
- Planning, all local embeddings/retrievals, fusion candidates, concurrency, elapsed time, and
  context are explicitly bounded to the measured 0199 envelope. One cancellation token propagates
  through planning and every retrieval and prevents a later generation call.
- Empty, malformed, duplicate-only, refused, timed-out, unavailable, or failed planning falls back
  to the unchanged single-query path. The preview reports requested strategy, applied strategy,
  planner/fusion version, bounded planned queries, and a typed sanitized fallback reason; fallback
  is never silent and never becomes a success-shaped multi-query result.
- Retrieval fingerprints bind the question, requested/applied strategy, normalized query plan,
  planner/fusion versions, and final evidence identities/generations. Generation repeats and
  reauthorizes retrieval and rejects stale confirmations exactly as the current flow does.
- Citation labels are assigned only after final fusion/packing. Prompt injection defenses,
  grounded-only behavior, optional distinguishable model knowledge, source-backed summary
  citations, stale/unavailable evidence, citation navigation, and saved-evidence deletion rules
  remain unchanged.
- `FileManagerService` remains a thin authorization/delegation facade. Planner/fusion orchestration
  lives in the RAG application capability, while the semantic worker remains an authorized,
  bounded local retrieval primitive and never receives LLM credentials or authority to plan scope.
- Preview/generate DTOs, generated HTTP client, `FileManagerClient`, Axum, Tauri, and mock adapters
  expose equivalent behavior. The Ask UI shows the strategy and fallback state accessibly without
  creating host-specific logic or a second frontend state mechanism.
- Tests cover schema migration/defaulting, opt-in disclosure, planner bounds and sanitization,
  per-query scope equality, threshold-before-fusion ordering, deterministic fusion/deduplication,
  final diversity/token packing, cancellation at every stage, all fallback categories, stale
  fingerprint rejection, citation stability, tenant isolation, diagnostics redaction, and
  HTTP/Tauri/mock parity. The 0199 benchmark is rerun against the production implementation and
  must still meet its recorded gate.

## Implementation Notes

- Primary ownership is `crates/fm-application/src/rag.rs`; extract a focused query-planning/fusion
  module if needed rather than growing `FileManagerService`. Keep
  `crates/fm-application/src/service.rs` to scope authorization and delegation, and update
  `crates/fm-application/src/rag_mapping.rs` for projections.
- Reuse `RagRetrievalCapability`, `AuthorizedRagRequest`, `RagCoordinator`,
  `LlmProfileService`, and the retrieval/context types in
  `crates/fm-semantic-worker/src/rag_retrieval.rs`. If ranked candidates must be exposed before
  final packing, extend the typed capability narrowly; do not duplicate Zvec access or
  authorization in the application layer.
- Add an enum/versioned fields in `crates/fm-transport-dto/src/rag.rs`, then regenerate
  `frontend/openapi/openapi.json` and `frontend/src/api/`; never hand-edit generated files.
- Update all three adapters under `frontend/src/api/client/` and the existing
  `frontend/src/features/semantic/rag-ask-dialog.ts` flow. Shared conversation state continues
  through the existing Meiosis-style application tree; do not introduce a new store.
- Normal diagnostics may record strategy/version, query count, candidate count, timing, and typed
  fallback category only. They must not record the question, planned query text, excerpts, prompts,
  filenames, paths, provider bodies, or credentials.
- No index, embedding-space, converter, or stored-vector migration is expected. Any discovered
  migration or retained-storage impact must return to the 0199 evaluation report before shipping.

## Agent Notes

- 2026-09-07 Copilot: Created as the implementation follow-up to 0199. The missing alternative
  description is explicitly interpreted as bounded query rewrite/decomposition plus deterministic
  fusion. Preserve `singleQuery` as the default and visible fallback, and revise this assumption
  here before implementation if the requester later provides a different design.
- 2026-09-07 Copilot: Implemented the typed strategy and fallback contract across the application,
  DTO/OpenAPI clients, mock adapter, and Ask UI. Planning receives only the exact question, retains
  it as the first query, accepts at most three bounded unique rewrites, and uses the selected
  profile's existing consent and host policy. Retrieval now exposes score-qualified primaries so
  deterministic RRF/deduplication precedes one final diversity, adjacency, and token-packing pass.
  Confirmation fingerprints bind the strategy, plan versions, normalized queries, and final
  evidence. Tauri Ask operations now propagate UI cancellation through a host-owned token.
- 2026-09-07 Copilot: Because 0199 correctly records **no-go** pending production-equivalent
  measurement, release builds keep this implementation disabled and visibly fall back to
  `singleQuery`. Developers may enable the backend with `PROCYON_ENABLE_MULTI_QUERY_RAG=1` and the
  UI with `VITE_ENABLE_MULTI_QUERY_RAG=true`; debug/test builds retain coverage without presenting
  the experiment as a measured improvement.
