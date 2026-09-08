# 0199 Benchmark bounded multi-query RAG retrieval

Status: done
Priority: medium
Subsystem: quality, search, rag
Depends on: 0183, 0186, 0188

## Context

The requested alternative grounded-RAG approach was not included in the handoff and the requester
is unavailable. This task therefore makes a conservative, revisable assumption: evaluate an
opt-in query-planning strategy that sends the current question to the already selected generation
profile, retains the original question, produces at most three bounded rewrite/decomposition
queries, runs every query against the same host-authorized semantic scope, and deterministically
fuses and deduplicates the ranked results before the existing diversity and context-budget rules
select evidence. The current exact-question retrieval remains the control and default.

Multi-query retrieval must earn its additional latency, endpoint disclosure, and resource use.
Generated-answer fluency is not evidence of retrieval improvement, so implementation must be gated
by a reproducible comparison using the retrieval metrics established in 0188.

## Acceptance Criteria

- A versioned evaluation corpus adds labelled single-intent, paraphrase, multi-facet/decomposition,
  multilingual, ambiguous, no-answer/negative-control, prompt-injection-shaped, duplicate, and
  scope-isolation cases. Every case records expected relevant file and chunk identities without
  private documents, paths, prompts, or model outputs.
- A deterministic benchmark runner executes the current single-query control and candidate
  multi-query strategy over the same corpus, authorized scopes, model/index/chunker/converter
  identity, score policy, rank cutoff, and cold/warm-cache conditions. Planner outputs are captured
  as explicit local fixtures or otherwise pinned so repeated comparisons do not depend on a live
  provider response.
- The candidate contract is fixed before scoring: one original plus at most three unique planned
  queries; bounded query bytes and planner output tokens; the existing absolute and strongest-hit
  relative score constraints applied independently to each query; deterministic reciprocal-rank
  fusion (or a documented deterministic alternative); stable identity-based deduplication and
  tie-breaking; then one final application of per-document diversity and context-token limits.
- The report compares file recall@10, chunk recall@10, MRR, nDCG@10, no-answer false-positive rate,
  duplicate rate, scope violations, query/embedding counts, selected context tokens, planner input
  and output tokens, and p50/p95 planning plus retrieval latency. Generated-answer wording is not a
  quality metric.
- The candidate receives a **go** only if scope violations remain zero, no-answer false positives
  do not increase, multi-facet file recall@10 improves by at least 0.10 absolute, no aggregate
  retrieval metric regresses by more than 0.02 absolute, and measured p95 planning-plus-retrieval
  latency stays within the documented four-query resource envelope. Otherwise the report records
  **no-go**, and 0200 is cancelled or left disabled rather than claiming an improvement.
- Adversarial cases prove that planned text cannot alter tenant/library/root/workspace filters,
  source restrictions, score thresholds, token budgets, or the number of retrieval calls. The same
  cancellation token stops planning and every retrieval.
- The checked-in report identifies exact baseline/candidate fingerprints, planner and fusion
  versions, benchmark hardware, migration/storage impact (expected to be none), metric deltas, and
  the go/no-go decision. Queries and labels remain local unless the user explicitly exports them.
- Focused tests reject incomparable runs, duplicate or unbounded planned queries, missing control
  observations, non-finite metrics, scope leakage, unstable fusion order, and reports that omit
  latency/resource measurements or the decision.

## Implementation Notes

- Extend `crates/fm-application/src/semantic_evaluation.rs` and
  `crates/fm-application/tests/fixtures/semantic-evaluation-v1.json` through an explicit schema
  migration; preserve existing 0188 file/chunk recall, MRR, and nDCG semantics.
- Exercise the real policy in `crates/fm-semantic-worker/src/rag_retrieval.rs`, especially
  `RagRetrievalPolicy::default_ask()` (`0.84` absolute floor, `0.02` strongest-candidate window,
  document/chunk diversity, adjacency, and the 8,192-token context budget). Do not lower or bypass
  these constraints to make the candidate appear better.
- The benchmark may use a production-independent planner fixture, but its result shape and bounds
  must be the same contract 0200 will implement. Do not add an unmeasured production code path in
  this task.
- Keep evaluation data local by default and use repository-owned/generated fixture documents. No
  telemetry, user documents, endpoint credentials, or raw provider payloads belong in fixtures,
  reports, or normal diagnostics.

## Agent Notes

- 2026-09-07 Copilot: Created from an incomplete handoff. The assumed candidate is an opt-in,
  bounded query rewrite/decomposition path using the selected generation profile, with the exact
  original question retained, per-query existing score constraints, deterministic fusion, and the
  current single-query path as the default control. Revise this assumption here before
  implementation if the requester supplies a different alternative.
- 2026-09-07 Copilot: Added the schema-v2 twelve-case corpus, bounded benchmark scoring and
  validation, deterministic control/candidate fixtures, and
  `docs/evaluations/multi-query-rag-v1.json`. The fixture exercises all required quality, safety,
  duplication, and resource metrics, but its timings and ranked observations are normalized
  regression data rather than a production-equivalent model/index run. The report therefore
  records **no-go** even though the fixture-level quality thresholds pass; a production measurement
  must replace it before enabling the candidate for normal users. No index, embedding, conversion,
  or retained-vector migration is required.
- 2026-09-08 Copilot: The requester restored the missing handoff. The intended alternative is
  Structured Knowledge Search, not LLM query rewriting. This experiment remains a valid no-go
  benchmark but is superseded product work; tasks 0201-0208 implement the actual local-first
  architecture.
