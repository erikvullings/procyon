# 0216 Knowledge retrieval relevance

Status: done
Priority: high
Subsystem: semantic search
Depends on: 0215

## Context

A production Search Knowledge query for `Explain the principles of Substance-Field modelling`
returns mostly irrelevant evidence. The retained index contains a directly relevant explanation
around pages 40-43 of *The Right Solution at the Right Time*, but generic occurrences of
`substance` and `field` elsewhere in the corpus can dominate the result list. Most rows in the
reported result also appeared to lack useful page navigation.

The existing task 0215 fix restores nested-span page labels, but relevance must be diagnosed
through the real hybrid retrieval path and corrected without weakening broad semantic search.
Adjacent retrieval context must also not be presented as though it were a matched section.

## Acceptance Criteria

- Reproduce the reported query against the retained production-shaped Zvec index and capture the
  full-text/semantic contribution trace.
- Rank a directly relevant Substance-Field modelling explanation ahead of generic documents that
  merely contain the separate words `substance` and `field`.
- Preserve deterministic, bounded hybrid retrieval and useful broad-query behavior.
- Add a regression that prevents adjacent retrieval context from being presented as a matched
  search section.
- Confirm every returned PDF evidence row with indexed page provenance exposes a page or page-range
  label through the frontend.

## Implementation Notes

- Diagnose `crates/fm-semantic-worker/src/knowledge_retrieval.rs`,
  `crates/fm-semantic-worker/src/zvec_storage.rs`, and the deterministic plan in
  `crates/fm-application/src/knowledge.rs` before changing weights or query expansion.
- Keep the retained developer index read-only during diagnosis.
- Do not require an LLM or network access.

## Agent Notes

- 2026-09-09 Copilot: Catalog inspection found 13 phrase/abbreviation-bearing chunks. The clearest
  relevant material is in *The Right Solution at the Right Time* on pages 40-43, while broad
  `substance`/`field` matches occur throughout unrelated sections. Building a real IPC retrieval
  probe next to identify whether query expansion, FTS token matching, semantic retrieval, RRF, or
  adjacency causes the observed ordering.
- 2026-09-09 Copilot: Rebuilt the current developer worker and reconciled the enrolled TRIZ root
  after finding that the retained index had been opened by an older converter pipeline. The
  restored model-scoped index contains 32 documents and 13,554 records.
- 2026-09-09 Copilot: Captured full-text, semantic, and hybrid traces for the exact reported
  sentence, the shorter subject, and the default Explain expansions. The current hybrid route
  ranks a direct Substance-Field explanation on page 40 first; the default expanded plan also
  keeps it first. Every primary result inspected carries exact or span PDF page provenance.
- 2026-09-09 Copilot: Identified page 176 as an adjacent context chunk for the actual page 177
  match. Search Knowledge now omits adjacent retrieval-only context from its document sections,
  while retaining it in the result set for grounded-answer generation. Added a grouping
  regression and verified the focused 97-test frontend suite.
