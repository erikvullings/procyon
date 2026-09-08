# 0205 Knowledge DSL and rule-based parser

Status: done
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
- 2026-09-08 Copilot: Added `fm-application::knowledge_dsl` (pure, no LLM/network dependency),
  exported from `lib.rs`. Implements a `field: value` DSL tokenizer (multiline and compact forms,
  quoted values with `\"`/`\\` escapes, comma-and-repetition-combining repeated fields, and
  documented aliases for every field and enum value) plus a separate deterministic
  natural-language rule matcher for find/definition/procedure/examples/limitations/evidence,
  apply/use/analyse, and comparison forms. A typed `KnowledgeQueryDraft` composer state mirrors the
  canonical/answer split exactly (`about`/`needs`/`related`/`scopes` vs. answer-only
  `action`/`context`/`constraints`/`format`), converts losslessly to/from
  `KnowledgeSearchRequest`/`KnowledgeAnswerRequest` given caller-supplied scope authorization and
  evidence fingerprint, and round-trips through `format_multiline`/`format_compact`. Unknown or
  misspelled fields and invalid need/scope/action/format values produce actionable diagnostics with
  byte-offset spans and Levenshtein-distance suggestions rather than silently guessing; ambiguous
  natural-language matches (e.g. "how to compare X and Y") report an explicit
  `KnowledgeParseConfidence::Ambiguous` plus the discarded alternative instead of dropping it.
  Verified 43 new focused tests (compact/multiline parsing, quoting/escaping, repeated fields,
  aliases, typo/invalid-value diagnostics, Unicode subjects, all round-trip directions, and
  do/to/constraint contamination regressions), all 552 passing `fm-application` library tests (one
  pre-existing ignored), `cargo fmt --check`, and warning-free `cargo clippy --all-targets -D
  warnings` for the crate. Left `Status: in_progress` and did not touch `TASKS/README.md` per
  reviewer request; frontend composer UI wiring is out of scope for this task.
- 2026-09-08 Copilot: Review hardening made the lexer escape-aware, rejects unterminated quotes,
  supports semicolon-delimited compact fields without spaces, preserves bare backslashes, and
  enforces canonical byte limits with checked request conversion. Natural `apply`/`use`/`analyse`
  forms now separate `to`/`for` application context from retrieval subjects and mark the split as
  ambiguous. Added lossless answer-depth handling and ensured maximum-sized valid canonical drafts
  remain parseable. Final verification covers 51 focused DSL tests, 11 planner tests, all 560
  passing `fm-application` library tests (one pre-existing ignored), formatting, and warning-free
  all-target clippy.
