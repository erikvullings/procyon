# 0204 Knowledge request model and deterministic planner

Status: open
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
