# 0328 Embeddings role consumer (cloud-capable semantic indexing)

Status: open
Priority: low
Subsystem: backend, semantic, settings
Depends on: 0322, 0184

## Context
0322 added the embeddings role (endpoint, model, allow-cloud flag) and its gate, but nothing reads it.
Semantic indexing still runs on the local semantic worker and never calls an LLM endpoint. Splitting
this out keeps 0322 to endpoint, role and consent configuration.

## Acceptance Criteria
- Semantic indexing resolves the embeddings role through `LlmProfileService::resolve_role`. With no
  role, or a role that is not ready, indexing stays on the local worker, as it does today.
- A cloud embeddings endpoint is used only when the role's `allowCloud` flag is set and consent is
  recorded for its host. Document content is sent only in that case, and the Settings copy says so.
- The index records the embeddings endpoint and model. A change of either marks the index stale and
  triggers reindexing; vectors from two models are never mixed.
- Browser, Tauri and mock hosts behave the same; provider calls stay in the Rust backend.
- Tests: role-driven selection, cloud gate, reindex on model change, no content sent without the flag.

## Implementation Notes
- Role resolution, consent and the cloud gate exist: `llm_profiles.rs` (`resolve_role`, `set_role`).
- Index metadata lives in the semantic data root; see 0184 and the semantic worker boundary notes in
  `CLAUDE.md`.

## Agent Notes
- 2026-11-12 claude: Split out of 0322 at the owner's request. Not started.
