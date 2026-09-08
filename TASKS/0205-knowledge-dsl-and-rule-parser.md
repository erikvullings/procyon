# 0205 Knowledge DSL and rule-based parser

Status: open
Priority: high
Subsystem: search, frontend
Depends on: 0204

## Context

Provide transparent, editable query input without requiring an LLM. The DSL and common
natural-language templates must parse into the canonical search and optional-answer models and show
the interpretation before execution.

## Acceptance Criteria

- Parse and format multiline and compact DSL with quoted values, commas, repeated constraints,
  whitespace, canonical fields, and documented aliases.
- Round-trip DSL, canonical models, and visual composer state without semantic loss, including
  optional answer fields when no LLM is available.
- Return actionable diagnostics for unknown/misspelled fields and invalid values.
- Deterministically recognize common find/definition/procedure/examples/limitations/evidence,
  apply/use/analyse, and comparison forms without an LLM.
- Separate subject/needs from application context and expose confidence/ambiguity rather than
  silently contaminating retrieval.
- Cover English plus locale-safe DSL behavior and extensive parser/formatter regressions.

## Implementation Notes

- Canonical retrieval fields: `about`, `need`, `related`, `scope`.
- Optional answer fields: `do`, `to`, `constraint`, `format`.
- An optional LLM parser is not part of this task and must never be a fallback dependency.

## Agent Notes

- 2026-09-08 Copilot: Created from restored Structured Knowledge Query phases 5 and 8.
