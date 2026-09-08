# 0206 Search-only knowledge UX and host parity

Status: done
Priority: high
Subsystem: frontend, backend, search
Depends on: 0205

## Context

Ship the complete local-first product: select indexed knowledge roots, compose subject and knowledge
needs, inspect the deterministic retrieval plan, execute search, group/diversify evidence, and open
exact sources. No LLM profile or answer panel may be required.

## Acceptance Criteria

- Add capability/root/parse/plan/execute APIs with equivalent HTTP, Tauri, mock, and
  `FileManagerClient` behavior.
- Provide `Search Knowledge…` entry points that default scope from the current indexed folder or
  semantic result set.
- Build an accessible visual composer for subject, needs, related terms, scope, and
  hybrid/full-text/semantic mode with an editable DSL equivalent.
- Show interpretation and advanced query preview, including fields explicitly not used for
  retrieval.
- Show grouped source results by knowledge need/document/relevance with reasons, structural
  provenance, fallback warnings, and exact source navigation.
- Adapt to independent FTS/vector/answer capabilities; search remains fully usable without an LLM
  and does not render a broken answer section.
- Cover keyboard, screen-reader, empty/loading/error/offline/fallback states, cancellation, host
  parity, and no-LLM end-to-end use.

## Implementation Notes

- Use existing Mithril, Meiosis-style state, localization, virtualized result surfaces, and client
  boundaries.
- Search is the primary action; answer generation is never automatic.

## Agent Notes

- 2026-09-08 Copilot: Created from restored Structured Knowledge Query phase 7.
- 2026-09-08 Copilot: Started after the canonical planner and deterministic DSL parser were
  committed. Implementation will preserve the existing `FileManagerClient` boundary and expose
  search independently from optional answer generation.
- 2026-09-08 Copilot: Implemented the backend/transport half. `fm-application::knowledge_search`
  composes the canonical `knowledge` planner, the `knowledge_dsl` parser, and the worker
  `knowledge_retrieval` capability behind an injectable `KnowledgeRetrievalCapability` (inert by
  default, worker-backed through `SemanticService`). Application code owns authorization:
  `SemanticLibraryService::resolve_knowledge_scope` reuses the same authorized-occurrence
  enumeration as Ask but never crosses the requesting workspace's tenant, and every returned
  evidence row is rechecked against that authorized source set (withheld rows are counted, not
  hidden). Retrieval always requests the privacy-safe trace internally so each result carries its
  planned-search reasons, route ranks, fused score, structural provenance, and stable source
  identity; the trace is only projected when the caller asked for it. Search works FTS-only with
  no LLM: capabilities report full text, semantic, and answer generation independently, hybrid
  requests keep explicit fallback metadata, and cancellation is coordinator-owned so HTTP and
  Tauri behave identically. Because the versioned worker protocol had no hybrid retrieval, it was
  extended rather than bypassed: new `KnowledgeSearch`/`GetKnowledgeCapabilities` RPCs, a
  `CAPABILITY_KNOWLEDGE_SEARCH` negotiation flag, `WorkerKnowledgeBackend` dispatch with
  cancellation registration, a client method, and `KnowledgeRetrievalService` wired into the
  managed/developer worker over the same catalog, model, and native Zvec index. Added
  `/api/v1/semantic/knowledge/{capabilities,roots,parse,plan,search,search/cancel,sources/resolve}`
  and the seven matching Tauri commands. Verified 3 IPC round-trip/no-path tests, 3 worker IPC
  transport tests, 5 coordinator tests, 6 service tests (no-LLM execute, withheld unauthorized
  evidence, denied root, cancellation, plan preview of excluded answer fields, roots/parse/source
  navigation), 1 scope-authorization test, 6 HTTP route/OpenAPI tests, 1 Tauri registration test,
  and the full fm-application (905), fm-semantic-worker (133), fm-server (333), fm-desktop (45),
  and fm-test-support (9) suites plus rustfmt and warning-free clippy. Frontend TypeScript and the
  generated OpenAPI/Orval artifacts were deliberately left untouched; `pnpm api:export` and
  `pnpm api:generate` still need to run.
- 2026-09-08 Copilot: Completed the search-only product and host parity. Added generated HTTP
  contracts plus equivalent HTTP, Tauri, and deterministic mock clients; a dedicated Mithril
  composer with lossless visual/DSL editing, revision-safe parse/plan/search handling, capability
  and fallback states, privacy-safe trace display, grouped bounded evidence, structural provenance,
  cancellation, and exact source navigation; command-palette, pane, keybinding, and native-menu
  entry points; and English/Dutch localization with fallback coverage for the other locales.
  Hardened authorization by filtering exact source sets before candidate budgets, globally fusing
  all root partitions before applying result/per-file/token limits, rejecting invalid scope
  selectors, and retaining only explicitly requested overlapping roots. Verified generated API
  stability, 2,489 Rust tests, 3 Rust doctests, 2,020 frontend tests, TypeScript, rustfmt,
  warning-free Clippy, Biome (pre-existing specificity warnings only), and a final combined
  correctness review.
