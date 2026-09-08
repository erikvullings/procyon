# 0208 Knowledge retrieval evaluation and release gate

Status: open
Priority: high
Subsystem: quality, search, release
Depends on: 0207

## Context

Evaluate whether structured hybrid retrieval improves source discovery over whole-question vector
search across representative domains without optimizing only for TRIZ.

## Acceptance Criteria

- Create a repository-owned multilingual corpus with specialist terms, procedures, examples,
  limitations, distracting application context, duplicates, updates/deletes, scope isolation, and
  negative controls.
- Compare whole-question vector, subject vector, structured subject/need vector, structured FTS,
  and structured hybrid retrieval on identical authorized content.
- Record Recall@5/10, MRR, relevant unique files, context-driven irrelevant hits, latency,
  query/embedding counts, storage, and migration impact with exact pipeline fingerprints.
- Prove the match-sorting regression: procedure/example sources rank strongly while a
  context-only sorting document does not.
- Run no-LLM parser/planner/FTS/vector/fusion/evidence integration tests and supported-platform FTS
  migration/lifecycle checks.
- Publish a go/no-go report; only a measured go may make Structured Knowledge Search production
  visible.

## Implementation Notes

- Do not include generated-answer fluency as a retrieval metric.
- Keep queries, source text, and judgments local unless explicitly exported.

## Agent Notes

- 2026-09-08 Copilot: Created from restored Structured Knowledge Query phase 9 and definition of
  done.
