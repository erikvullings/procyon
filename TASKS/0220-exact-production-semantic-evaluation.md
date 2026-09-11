# 0220 Exact-production semantic evaluation

Status: done
Priority: high
Subsystem: semantic, quality, release
Depends on: 0188, 0195, 0196, 0218

## Context

Task 0198 cannot qualify the production semantic distribution from developer-bundle tests,
synthetic rankings, or fixture-only metrics. It needs a reproducible runner that indexes the
repository-owned task-0188 corpus through the exact packaged worker, native runtime, model,
converter, chunker, retrieval policy, and index candidate, then records immutable, fail-closed
quality evidence without publishing semantic assets or changing either release-qualified variable.

## Acceptance Criteria

- Define repository-owned or deterministically generated source documents for every labelled
  task-0188 case, including authorized scope, excluded scope, lifecycle state, and structural
  citation expectations, without private user data.
- Run the corpus offline through the exact content-addressed production worker and packaged Zvec
  and ONNX Runtime bytes where applicable; reject missing, substituted, drifted, or non-production
  artifacts and fingerprints.
- Record aggregate and per-case file recall@10, chunk recall@10, MRR, nDCG@10, negative-control
  false-positive rate, and deterministic grounded-answer citation correctness.
- Bind reports to the immutable Procyon revision, model/revision/tokenizer, converter, chunker,
  retrieval thresholds and caps, worker protocol, index schema, Zvec and ONNX Runtime identities,
  corpus digest/schema, and package/catalog artifact identities.
- Reject missing or duplicate observations, unknown cases, malformed ranks or citations, scope
  leakage, identity drift, stale or tampered evidence, and reports not produced from the
  production package.
- Retain the `0.84` absolute floor and `0.02` relative window unless measured before/after evidence
  justifies a change; document threshold, storage, and migration impact explicitly.
- Keep raw queries/source content and full evidence private; check in only an operator-readable
  NO-GO template or aggregate/opaque per-case report and a validator that fails closed.
- Wire genuine production evidence into the semantic release precondition without enabling
  `SEMANTIC_RELEASE_QUALIFIED` or `KNOWLEDGE_SEARCH_RELEASE_QUALIFIED` and without adding a
  workflow-dispatch publication path.
- Add deterministic metric, corpus/report identity, missing/extra case, negative-control,
  citation, package-drift, and stale/tampered-report tests.

## Implementation Notes

- Reuse the task-0188 metric implementation and the task-0208 release-report validation pattern,
  but do not use task-0208 surrogate retrieval as production evidence.
- Build or acquire packages only through the existing pinned and checksum-verified production
  bundle machinery. Runtime evaluation itself must have no network authority or telemetry.
- Task 0198 remains blocked after this slice until its unrelated installed lifecycle,
  accessibility, privacy, failure-mode, and release-owner gates are complete.

## Agent Notes

- 2026-09-10 Copilot: Split this implementation slice from blocked task 0198 after private run
  `34509441435` produced all four supported production payloads. Started from clean
  `origin/main` revision `518a9d00ad9f057ba708f461fb86ff5d3c170818`; neither release-qualified
  variable will be changed.
- 2026-09-11 Copilot: The local macOS arm64 exact-package dry run completed against the packaged
  worker/model/Zvec bytes and production Ask policy. It measured file/chunk recall@10 `0.958333`,
  MRR `0.916667`, nDCG@10 `0.967762`, zero negative-control false positives, offline citation
  correctness `1.0`, and offline citation recall `0.923077`. The unsigned dirty-tree report is
  private and explicitly non-production; summary, unavailable-source, concept-label, generated
  answer, and unrelated task-0198 gates remain blocked.
- 2026-09-11 Copilot: Private run `34630169354` passed the exact packaged evaluator on every
  supported target from clean revision `eef47b66fe9147131d41a108636d003011edb9b4`. The four
  target reports produced identical quality metrics and were retained as private Actions artifacts;
  a locally revalidated private aggregate has SHA-256
  `ad6061f4824878ddaf44117c483545717276eced529dd461da637e02cb7ca524`.
  The implementation slice is complete, but its measured decision is NO-GO and task 0198 remains
  blocked on the explicitly recorded specialized, generated-answer, lifecycle, accessibility,
  privacy, failure-mode, and release-owner evidence.
