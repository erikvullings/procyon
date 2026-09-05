# 0191 Real multilingual model in the semantic developer bundle

Status: done
Priority: high
Subsystem: desktop, semantic, packaging
Depends on: 0178, 0181, 0190

## Context

Task 0190 built an installable developer bundle, but mapped every `SemanticProfile` to one
deterministic 384-dimensional token-hashing fixture. That proves the plumbing and nothing else:
selecting **Multilingual quality** installed a few hundred bytes of metadata and produced
retrieval that cannot distinguish meaning, so a developer cannot judge whether semantic search
works at all. The bundle needs a real, downloadable, multilingual embedding model behind that
profile without weakening the signed-catalog, no-network-in-the-worker, host-owned-path, or
explicit-consent contracts, and without implying that the developer bundle is a production
component pack.

## Acceptance Criteria

- Selecting **Multilingual quality** offers, downloads, verifies, and installs a substantial real
  multilingual embedding model through the existing consent and checksum lifecycle; its offered
  download and disk figures are the model's real size.
- The compact profiles keep the zero-download deterministic fixture so the pipeline stays testable
  offline and in ordinary unit runs.
- The worker loads whichever model the host installed and activated, rather than always the
  hashing embedder, and its identity is derived from the installed artifact.
- Model bytes are pinned to one immutable upstream revision and verified by exact size and SHA-256
  before they are packed; a mismatch fails the build rather than producing a bundle.
- The worker performs no network access. The inference runtime is part of a catalog-verified
  artifact rather than an ambient shared library that happens to sit in a build directory.
- Model paths reaching the worker are resolved by the trusted host from durable component state;
  nothing frontend-reachable contributes a path, and relative or missing paths are refused.
- The asymmetric query/passage input roles the model requires are applied at the right seams.
- Switching to a different model rebuilds the enrolled-root indexes it invalidated: the worker
  holding the previous model is stopped and its connection dropped, then every available enrolled
  root is reconciled into the newly active model. Durable activation survives a failed pass, and no
  failure is silent.
- The pre-model-scoped index layout left behind by task 0190 is retired rather than silently
  reused, so nobody assumes its hash-space contents still answer queries.
- Focused tests cover catalog profile resolution and offer size, offline loading and inference,
  cross-language ranking, launch-argument path safety, activation policy, and post-migration
  reindex orchestration, without requiring the large download in ordinary unit runs. The
  real-model tests are `#[ignore]` and are run by an explicit documented command.
- Documentation records the model, pinned revision, license, download/disk/RAM cost, caching, and
  what selecting the profile actually does.

## Implementation Notes

- Model: `intfloat/multilingual-e5-small`, MIT, pinned to Hugging Face revision
  `614241f622f53c4eeff9890bdc4f31cfecc418b3`; 384 dimensions, 512-token window, ~465 MiB.
- The managed installer stores exactly one payload file per artifact, so the model ships as a
  single deterministic *model pack*: a bounded self-describing index followed by the concatenated
  member files, with a SHA-256 per member. This keeps the whole model one checksummed,
  catalog-signed artifact and lets the worker read it offline without unpacking a second copy.
- Inference uses `ort` (ONNX Runtime) plus `tokenizers`, both behind the opt-in `developer-bundle`
  feature. ONNX Runtime links statically into the worker executable, so the packaged worker has no
  extra dynamic-library dependency beyond the already-cataloged Zvec runtime.
- Reuse the existing `CpuEmbeddingLoader`/`LocalEmbeddingRuntime` seam rather than adding a second
  embedding path; the pack selects which loader is constructed.
- Keep `--developer-data-dir` and the new `--developer-model-pack` host-only, resolved lazily at
  launch so a model installed after startup is picked up without restarting.

## Agent Notes

- 2026-09-05: Added `fm-semantic-components::model_pack`, a deterministic single-file pack format
  (magic, bounded JSON index, per-member digests, contiguous payload). Both developer models are
  packed, so the worker has one loading path and the host one activation contract. Byte-identical
  output for identical inputs is covered by test.
- 2026-09-05: Added `scripts/fetch-semantic-model.mjs` and `pnpm semantic:model:fetch`. It
  downloads each pinned file with progress into `target/semantic-model-cache/...`, verifies exact
  length and SHA-256, and skips already-verified files. `pnpm semantic:bundle:dev` calls it before
  the Rust build so a bad or missing download fails in seconds rather than after a full ONNX
  Runtime compile.
- 2026-09-05: The developer catalog now carries four artifacts and two model manifests. Compact
  multilingual and Compact English resolve to the hashing fixture; Multilingual quality resolves to
  the packed E5 model, whose catalog resources report the real 465 MiB download/disk and a
  conservative 1,600 MiB peak-RAM estimate. That single figure is declared once in the bundle
  builder and quoted verbatim by the operations documentation.
- 2026-09-05: Added `developer_onnx.rs`: mean-pooled transformer inference over the installed pack,
  bounded intra-op threads, 512-token truncation, batch padding, and validation that the graph's
  inputs/outputs and output width match the declared identity. `token_type_ids` is supplied only
  when the graph declares it.
- 2026-09-05: The worker resolves its model from the pack the host passes on `--developer-model-pack`,
  rejects relative paths and packs marked production, derives its identity and index manifest from
  the pack, and gives each model its own catalog/Zvec directory so switching profiles starts a fresh
  index instead of colliding embedding spaces. E5's `query: `/`passage: ` prefixes are applied by a
  thin role wrapper around the shared embedding runtime, so search and ingestion each get the right
  role and a model declaring no prefixes is passed through unchanged.
- 2026-09-05: The desktop host resolves the active model pack lazily from durable component state
  at worker-launch time, and the connector accepts only an absolute path to an existing regular
  file. Activation now validates the pack header, refuses a production-marked pack or an identity
  that disagrees with the signed catalog, and re-hashes only small members so activating a
  multi-hundred-megabyte graph stays fast.
- 2026-09-05 (review follow-up): Completing a model migration in the debug host now runs an
  explicit reindex pass. `SemanticCapability` gained a defaulted `restart`, implemented by
  `IpcSemanticCapability` as "stop the worker that is currently serving, then drop the cached
  client"; `WorkerConnector::connect_existing` makes that discovery-only so nothing is launched
  merely to be shut down. The orchestration lives in the new `semantic_model_change` module —
  restart first, then reconcile each available enrolled root — and returns a report the desktop
  command logs per root. Consent and activation are never rolled back by a failure.
- 2026-09-05 (final review): Restart now waits for the old worker endpoint and process marker to
  disappear before reindexing, and a real subprocess test covers that ordering. The worker records
  the active model identity durably beside the indexes and clears the newly selected model's prior
  index whenever that identity changes, so switching back cannot expose deleted documents or roots
  whose consent was revoked, even if the host exited between activation and worker launch. A failed
  restart aborts the feed and reports every affected root.
- 2026-09-05 (review follow-up): The worker retires a task-0190 flat `catalog.sqlite`/`zvec` pair
  to `superseded-flat-index/` on first open and says so on stderr, rather than reusing or deleting
  a superseded embedding space.
- 2026-09-05 (review follow-up): The two real-model tests are `#[ignore = "requires
  PROCYON_SEMANTIC_MODEL_PACK from a built developer bundle"]` and now panic with actionable text
  instead of returning success when the variable is absent. `pnpm semantic:model:verify` resolves
  the pack from the built bundle, sets the variable, and passes `--ignored`.
- 2026-09-05: Verified on Apple silicon: real bundle build (465 MiB pack), `otool -L` confirming the
  worker links no ONNX Runtime dylib, and a gated end-to-end run in which an English query retrieves
  the relevant Dutch document through real ingestion and search. Windows and Linux packaging follow
  the same static-linking path but were not exercised on this host.
- 2026-09-05 (final verification): The Rust builder now independently checks exact pinned cache
  lengths and SHA-256 values, then streams the completed pack members against those same pins before
  signing the catalog. `pnpm run lint`, 845 affected-package tests, the real worker shutdown test,
  and both `pnpm semantic:model:verify` E5 tests pass.
- 2026-09-05 (cold-start follow-up): Developer worker launch now allows a bounded 30 seconds for
  the real model to load before binding IPC, while ordinary worker launch keeps its two-second
  bound. A real subprocess test delays binding for three seconds and proves the connector waits.
- 2026-09-05 (crash-recovery follow-up): Activation writes a durable pending-reindex marker before
  changing the selected model. Its unique migration generation clears the target index exactly once,
  so a replacement worker preserves roots already rebuilt during the same pass. Desktop startup
  resumes reconciliation after a host crash, and only a complete reindex removes the marker. A new
  developer host also replaces a discoverable worker from the previous host so it cannot keep
  serving the pre-activation model; an already-stopped stale cached worker no longer aborts reindex.

## Validation commands

```bash
pnpm semantic:bundle:dev     # fetch/verify the pinned model, then build the bundle
pnpm semantic:model:verify   # run the #[ignore]d real-model tests against that bundle
cargo nextest run -p fm-application --test semantic_model_change_reindex
cargo nextest run -p fm-semantic-worker --features developer-bundle
```
