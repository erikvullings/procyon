# 0248 Gemma non-experimental qualification tooling

Status: open
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
5. Gemma vs E5 quality on user-written queries.

These need manual runs on the release owner's own data. This task provides the tooling and the
procedure. Promotion itself stays open until the
[checklist](../docs/qualification/gemma-non-experimental-checklist.md) results are recorded and
approved.

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
- [ ] Release owner records results for items 1–5 and decides on promotion.

## Implementation Notes

- Backend selection is `GemmaBackendSelection` in
  `crates/fm-semantic-worker/src/developer_bundle.rs`. The host override is applied in the
  `WorkerConnector` spawn path (`ManagedWorkerLaunch::with_host_cpu_override`), so an
  already-running worker is unaffected until it exits.
- PDF spools are `tempfile::tempfile()` in the OS temp directory: the ingestion spool (200 MiB
  `MAX_PDF_INGESTION_BYTES`) and the staged visual pages (256 MiB `MAX_STAGED_VISUAL_BYTES`). They
  are unlinked on Unix, so the sampler reads open descriptors through `lsof` or `/proc`.
- The existing `semantic_production_evaluation` example scores a fixed embedded corpus. It is not
  suited to user-written queries, hence the separate comparison script.

## Agent Notes

- Found while writing the checklist: `fm-server` builds `FileManagerService` with an unavailable
  semantic capability and an unconfigured administrator-provisioned library. Item 3 is therefore
  expected to be **Blocked** until the server can attach to a provisioned worker, or the release
  owner scopes browser hosts out for Gemma. The checklist includes a precondition check.
- The real worker has not yet been run with the sampler on a large scanned PDF in this change.
  A 155 MiB scanned PDF is available locally for the item-1 run. The sampler was verified against a
  process holding an unlinked 5 MiB temp file.
