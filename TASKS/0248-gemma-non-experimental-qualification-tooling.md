# 0248 Gemma non-experimental qualification tooling

Status: in_progress
Priority: high
Subsystem: semantic
Depends on: 0244

## Context

The opt-in EmbeddingGemma 2 package shipped as experimental in v0.5.0-2
([release notes](../docs/evaluations/embeddinggemma-experimental-release-2026-10-15.md)). Five items
are unqualified:

1. resource ceilings: peak worker RSS and the peak temp disk of anonymous PDF spools;
2. CPU fallback on a GPU-capable machine;
3. cross-host citation navigation (fm-server and browser against the desktop library);
4. upgrade and rollback from v0.5.0-2, including switching to E5;
5. on-device Gemma image/audio/video search, source attribution, and consent isolation.

These qualification runs remain manual and were not completed. On 2026-10-18
the release owner explicitly chose to ship v0.6.0 through the separate
risk-accepted path rather than wait for five-gate qualification. This task
records the tooling and release decision; risk acceptance does not change the
checklist results or imply qualification.

## Acceptance Criteria

- [x] An explicit, default-off CPU-only switch: the `PROCYON_GEMMA_FORCE_CPU` host environment
  variable becomes the `--gemma-force-cpu` worker flag on every managed Gemma launch, so it reaches
  the installed desktop app's child worker. Production workers accept the flag. It cannot be
  combined with the development `--gemma-cpu-images` baseline.
- [x] Forced CPU is reported: `gemma-backend` and the worker description carry
  `[CPU forced by PROCYON_GEMMA_FORCE_CPU]`, and Settings shows "Last worker launch: CPU (forced by
  PROCYON_GEMMA_FORCE_CPU)".
- [x] Tests cover value parsing, launch arguments, CLI validation, desktop device mapping, and (in
  the ignored real-model Metal test) zero Metal dispatches when forced.
- [x] `scripts/measure-semantic-worker-resources.mjs` samples the worker tree's RSS, the open
  temp-file bytes (including unlinked spools), and free disk. It writes `samples.csv` and
  `summary.json` and records no names or content. Its parsers and summary are unit tested.
- [x] `scripts/semantic-quality-compare.mjs` scores user-recorded top-k paths for two models
  (hit@k, MRR, recall@k, misses, head-to-head), without reading files or using the network. Unit
  tested.
- [x] The checklist documents the procedures, pass criteria, the spool location, the privacy rules,
  and a fill-in results table.
- [x] Item 5 qualifies locally consented image/audio/video retrieval and truthful file-level
  provenance; published text benchmarks remain context, not an on-device E5 comparison gate.
- [x] The release owner either records all five qualification results and
  approves the qualified path, or records a separate explicit risk acceptance
  for an unqualified release. Risk acceptance preserves actual failed/missing
  results and must never label them `Pass`.

## Implementation Notes

- Backend selection is `GemmaBackendSelection` in
  `crates/fm-semantic-worker/src/developer_bundle.rs`. The host override is applied in the
  `WorkerConnector` spawn path (`ManagedWorkerLaunch::with_host_cpu_override`), so an
  already-running worker is unaffected until it exits.
- PDF spools are `tempfile::tempfile()` in the OS temp directory: the ingestion spool (200 MiB
  `MAX_PDF_INGESTION_BYTES`) and the staged visual pages (256 MiB `MAX_STAGED_VISUAL_BYTES`). They
  are unlinked on Unix, so the sampler reads open descriptors through `lsof` or `/proc`.
- The existing `semantic_production_evaluation` example scores a fixed embedded corpus. It is not
  suited to user-written queries. The comparison script remains available for optional text
  evaluation, but it is not an item-5 gate.

## Agent Notes

- Found while writing the checklist: `fm-server` builds `FileManagerService` with an unavailable
  semantic capability and an unconfigured administrator-provisioned library. Item 3 is therefore
  expected to be **Blocked** until the server can attach to a provisioned worker, or the release
  owner scopes browser hosts out for Gemma. The checklist includes a precondition check.
- The real worker has not yet been run with the sampler on a large scanned PDF in this change.
  A 155 MiB scanned PDF is available locally for the item-1 run. The sampler was verified against a
  process holding an unlinked 5 MiB temp file.
- 2026-10-16: Replaced the item-5 text comparison gate in
  `docs/qualification/gemma-non-experimental-checklist.md` with nine local media-search probes,
  source/coverage checks, and an unenrolled/excluded-source check. Existing Gemma libraries can
  enable audio/video in Settings (0247) without resetting the dimension. Search presents file-level
  media evidence; video sampled-frame coverage is not timestamp navigation. No installed-app
  result or promotion decision is asserted here.
- 2026-10-18 Copilot: Updated the release decision per the user's explicit
  direction for v0.6.0. Manual gates remain unqualified (Metal resource gate
  failed; CPU resource and gates 2–5 remain unfilled). The separate
  risk-accepted path is distinct from the five-gate qualified path, requires
  owner acceptance of the listed limitations, and does not synthesize
  qualification evidence. See
  `docs/evaluations/embeddinggemma-risk-accepted-release-2026-10-18.md`.
- 2026-10-18 Copilot: Implemented the separate candidate-bound risk-acceptance
  contract, signed four-target catalog/artifact checks, source-lineage checks,
  release-workflow gates, and explicit Settings disclosure. The existing
  five-gate checker remains strict. Focused release tests (48), Settings tests
  (42), frontend typecheck, and repository lint passed. The task stays
  in-progress until a new post-merge signed candidate is bound to the actual
  release-owner approval manifest; no existing candidate is reused.
