# 0208 Knowledge retrieval evaluation and release gate

Status: done
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
- 2026-09-08 Copilot: Added a repository-owned 18-document, 32-chunk multilingual corpus and a
  five-strategy harness over the production planner, coordinator, catalog, fusion, and evidence
  materialization paths. The checked-in report records an honest NO-GO: structured FTS and hybrid
  retrieval improve Recall@5 and contextual-noise behavior, but Recall@10 is saturated and no
  production/cross-platform measurement exists. The report records source-, corpus-, pipeline-,
  storage-, migration-, query-, embedding-, latency-, scope-, deletion-, and match-sorting
  evidence. Storage counts every indexed occurrence, including duplicates.
- 2026-09-08 Copilot: Added a compile-time fail-closed release gate across application capability
  discovery and plan/search/answer execution. Qualified desktop builds require an exact protected
  repository variable, an internally recomputed measured GO report bound to the current corpus and
  release-critical retrieval sources, and eight separately executed native Zvec FTS
  migration/restart/recovery checks on each supported platform. Developer and test builds remain
  available without making unqualified release builds visible.
- 2026-09-08 Copilot: Verified 2,557 workspace Rust tests and doctests, 2,065 frontend tests, 9
  release-profile evaluation tests, 8 native Zvec lifecycle checks, 17 desktop/release script
  tests, rustfmt, warning-free Clippy, Biome (pre-existing specificity warnings only), and final
  release-integrity review. The full script suite has one unrelated pre-existing failure because
  `scripts/architecture-docs.test.mjs` expects 11 ADRs while the repository currently contains 12.
