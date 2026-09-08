# 0204 Knowledge request model and deterministic planner

Status: done
Priority: high
Subsystem: domain, semantic, search
Depends on: 0203

## Context

Introduce the canonical retrieval-only request and a deterministic planner. A subject is the
strongest signal; knowledge needs select conservative retrieval templates; related terms explicitly
expand search; optional action/context/output fields remain outside retrieval.

## Acceptance Criteria

- Add typed `KnowledgeSearchRequest`, extensible subjects/scopes, `KnowledgeNeed`,
  `RetrievalMode`, options, plan/search/priority/reason models, capabilities, and validation.
- Model optional action/context/constraints/output separately as `KnowledgeAnswerRequest`.
- Always include the raw subject search; conservatively expand explicit needs and related terms;
  deduplicate equivalent searches while combining reasons.
- Keep `do:`/`to:` out of retrieval and use action-to-need mappings only as defaults when no needs
  were explicitly selected.
- Keep application authorization/composition outside the worker and LLM parsing outside the
  deterministic planner.
- Cover bounds, stable plan order, aliases at the boundary, multi-subject extensibility, and the
  match-sorting contamination regression.

## Implementation Notes

- Initial needs: overview, definition, procedure, examples, evidence, arguments, comparison,
  limitations, references.
- Initial priorities: primary subject, secondary needs, related terms.

## Agent Notes

- 2026-09-08 Copilot: Created from restored Structured Knowledge Query phases 4 and 6.
- 2026-09-08 Copilot: Started after 0203 established retrieval routes, evidence, and publication
  snapshot materialization. The planner will live in `fm-application`, keeping worker retrieval
  mechanical and host authorization/composition above the worker boundary.
- 2026-09-08 Copilot: Added `fm-application::knowledge` with bounded canonical search and
  answer-only requests, extensible multi-subject and authorized-scope models, typed needs/actions,
  independent FTS/vector/answer capabilities, and a deterministic versioned plan. Planning emits
  raw subjects first, then conservative need expansions and explicit related terms; equivalent
  searches retain their first spelling and combine reasons, and omitted bounded expansions are
  reported. The planner accepts only a typed action, making answer context, constraints, output,
  and evidence text structurally unavailable to retrieval; explicit needs always override action
  defaults. Added canonical worker-reason and capability bridges without moving authorization into
  the worker. Verified 10 focused planner tests, all 509 passing `fm-application` library tests
  (one pre-existing ignored), formatting, and warning-free clippy.
