# 0206 Search-only knowledge UX and host parity

Status: open
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
