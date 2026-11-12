# 0322 LLM endpoints and roles in Settings, consent at configuration

Status: open
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
