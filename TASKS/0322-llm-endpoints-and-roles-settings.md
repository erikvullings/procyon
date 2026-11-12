# 0322 LLM endpoints and roles in Settings, consent at configuration

Status: in progress (roles, migration, consent, cloud-embedding gate, own Settings section and simple/advanced view done; embeddings consumer and reindex deferred)
Priority: medium
Subsystem: backend, frontend, settings
Depends on: 0184, 0243

## Context
LLM connection profiles (0184) live under the semantic-search settings, and cloud consent is
requested at point of use. Consent should be a one-time decision made when an endpoint is
configured, not a prompt during chat. The AI configuration should also stop being a semantic-search
sub-feature: chat (0324), semantic search, image generation (0325) and natural-language search (0326)
all need an LLM.

## Acceptance Criteria
- Settings > LLM (AI) replaces the current location. It has two parts:
  - **Endpoints**: a list of connections (preset, base URL, API key in the OS keychain, TLS policy,
    test result). Add, edit, test, remove.
  - **Roles**: each role references one endpoint and one model. Roles: chat, embeddings (semantic
    search), and reserved entries for later features (image generation, natural-language search).
- Three configurations are expressible from the same screen, and the UI shows only the simplest
  until the user asks for more:
  A. one endpoint, one model for all roles;
  B. one endpoint, separate chat and embedding models;
  C. several endpoints, each role routed to any of them.
- Per-role "allow cloud endpoints" flag. Semantic indexing defaults to local-only: a cloud endpoint
  cannot be assigned to the embeddings role until the user enables the flag, with an explanation
  that document content would be sent.
- Consent is requested in the endpoint editor when a cloud host is saved: host name, "prompts and
  file content leave this machine", Allow or Cancel. Consent is stored on the endpoint with the host
  it was given for. Changing the host or path re-prompts. Local endpoints (loopback, private LAN)
  need no prompt.
- Chat, semantic search and the file assistant never prompt for consent. If a role points at an
  endpoint without consent, the feature shows "endpoint needs consent in Settings" with a link.
- Existing single-profile configurations migrate to scenario A without loss; the active profile
  becomes the chat and embeddings role.
- Browser and Tauri hosts behave the same; provider calls stay in the Rust backend.
- Tests: migration, role resolution, consent invalidation on host change, cloud-embedding gate.

## Implementation Notes
- Current consent model: profile stores `consented_host`; `LlmProfileService::generate` returns
  `ConsentRequired` when it does not match; `activate(id, consent)` records consent. Reuse this.
- Settings UI lives in `frontend/src/features/settings/llm-profile-management.ts`; the semantic
  settings currently host it.
- Move the document into the settings model via a versioned migration (settings migration
  machinery, see 0030).
- Open decision: whether roles are stored per workspace or globally. Recommendation: global, with
  the roles of the semantic index recorded in the index metadata so a change of embedding model
  triggers reindexing rather than mixing vectors.

## Agent Notes
- 2026-11-12 claude: Created from design discussion. Not started.
- 2026-11-12 claude: Implemented the backend and transport slice. `LlmProfilesDocument` schema 5 adds
  `roles` (chat, embeddings: profile, model, allowCloud). Schema 4 migrates the active profile into
  both roles with allowCloud false, so cloud embeddings stay blocked until the person allows them.
  `set_role` refuses a cloud endpoint for embeddings without the flag; `resolve_role` returns the
  endpoint and model or `ConsentRequired`/`CloudEmbeddingNotAllowed` and never prompts;
  `grant_endpoint_consent` records consent for the saved host; deleting a profile clears its roles.
  Routes `GET/PUT/DELETE /api/v1/llm-roles[/{role}]` and `POST /api/v1/llm-profiles/{id}/consent`,
  Tauri commands and the HTTP/Tauri/mock adapters. Tests cover migration, cloud-embedding gate,
  chat consent, host-change invalidation and local endpoints (llm_profiles.rs).
- 2026-11-12 claude: Frontend: `llm-role-management.ts` renders the roles section (endpoint, model,
  allow-cloud flag for embeddings) under the endpoint editor. Saving a cloud host requires the consent
  switch and grants consent after the save; `models/llm-roles.ts` holds the pure rules, tested.
- 2026-11-12 claude: Open. (1) "Settings > LLM (AI)" is still a sub-section of Semantic settings; it
  is not yet a top-level page. (2) The UI shows the full role form; the simple/advanced split
  (scenario A vs B vs C) is not built. (3) Only chat and embeddings roles exist; image generation and
  natural-language search are not reserved in the UI. (4) No consumer reads the embeddings role yet:
  semantic indexing remains local and does not call the endpoint, so the cloud-embedding gate has no
  caller. (5) RAG and the file assistant still use the saved profile chosen at call time, not the chat
  role. (6) Index metadata does not yet record the embeddings role, so a model change does not
  trigger reindexing. (7) Roles are stored globally (the recommended option); the per-workspace
  question is unresolved.
- 2026-11-12 claude: Code review fixes: consent resets when the host changes in the editor; a
  dangling role is pruned on load instead of failing the whole document. Open from review: roles
  status is loaded once on mount, so it is stale after a save or consent (refresh needed); a consent
  grant that fails after a successful save leaves the profile saved without consent; no HTTP route
  tests for `/api/v1/llm-roles`; the frontend `endpointIsLocal` duplicates the backend locality rules
  (the draft should carry the backend's classification); the roles section refetches profiles.
- 2026-11-12 claude: Follow-up pass. The roles section reloads when an endpoint changes (settings
  revision counter), so status is no longer stale. "LLM (AI)" is its own Settings section, mounted
  always like Semantic so its save handler survives section changes. Roles show a simple view (chat
  only; saving also binds embeddings to the same endpoint and model when it is local) with an
  advanced switch that reveals the embeddings role. The file assistant and the RAG answer dialog start
  on the chat role's endpoint (`preferredProfile`). HTTP route tests added
  (`apps/fm-server/tests/llm_role_routes.rs`). Still open: embeddings consumer (deferred to a later
  task, per the owner); index metadata and reindex on embeddings-model change; the endpoint-local
  classification is duplicated in TypeScript.

