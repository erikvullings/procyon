# 0239 Optional EmbeddingGemma 2 semantic package

Status: in_progress
Priority: medium
Subsystem: backend, frontend, semantic, release
Depends on: 0195, 0196

## Context

Keep the existing multilingual-E5-small package and its text-search behavior as
the default. Offer `google/embeddinggemma-2` as a separately installed,
explicitly selected advanced semantic package for text, code, and optional
image search. Audio and video may also be enabled by the user. This is a
different embedding space, not an upgrade that can reuse E5 vectors.

The exploratory comparison on branch `embedding-model-comparison` (commits
`91801c3` and `4f1bf93`, `docs/evaluations/embedding-model-comparison.md`)
found no quality advantage on small, easy text/code fixtures. Its Python CPU
measurements found substantially slower inference and higher memory use than
E5; Apple MPS improved throughput, particularly for images. Neither the
fixtures nor that Python runtime qualify a production package. Google's
published code improvement compares EmbeddingGemma 2 with EmbeddingGemma 1,
not E5. Re-evaluate the decision with representative, independently labelled
code and image workloads rather than treating the exploratory tie as proof
that the models are equivalent.

Sources: [EmbeddingGemma 2 model card](https://huggingface.co/google/embeddinggemma-2)
and [developer guide](https://developers.googleblog.com/embeddinggemma-2-the-developer-guide/).

## Acceptance Criteria

- Keep E5 installed/available as the default. Offer Gemma as an optional,
  separately verified package with clear model/license, download, disk, peak
  RAM, CPU/GPU availability, and full-reindex costs before activation. Never
  infer GPU support from the model card alone. Ordinary file-manager operation
  must remain unaffected when Gemma is not installed.
- Pin the exact model revision and all required tokenizer/processor/runtime
  bytes; package and verify them through the existing signed catalog and
  offline worker lifecycle. Qualify CPU inference on every supported target.
  Offer GPU acceleration only on targets with verified runtime, output parity,
  resource limits, and CPU fallback; do not use FP16, which the model card
  warns can yield invalid or degraded embeddings.
- At library initialization, select one supported output dimension
  (128/256/512/768) and separate opt-in checkboxes for images, audio, and
  video. These choices are immutable for that library; do not expose controls
  that change them after initialization. Any future change path must explicitly
  confirm and rebuild affected vectors and indexes.
  Video requires the vision encoder even if standalone image indexing is off.
  Explain the quality/storage trade-off, especially for 128-dimensional media
  search. Truncate and L2-normalize query and corpus vectors identically.
- Route search queries, Ask questions, and code queries through their
  appropriate model-owned text prompts (`SearchQuery`,
  `QuestionAnswering`, `CodeRetrieval`), while indexing document text with
  the document format (including a real title when available). Embed media
  without text-only prefixes. Keep search intent explicit and measure whether
  shared document vectors serve each query type adequately; preserve existing
  E5 semantics for E5 libraries.
- Add bounded, cancellable image, audio, and video ingestion only when each
  modality was enabled, with safe decoding/sampling, source-revision caching,
  timestamps for temporal evidence, and honest unsupported/partial coverage.
  Keep provider access, consent, exclusions, tenant filters, and source
  authority in Procyon rather than granting the worker filesystem traversal
  or network access. Do not conflate semantic image similarity with exact or
  perceptual duplicate detection in 0170.
- Starting a Gemma library may begin with an empty index rather than migrating
  an existing E5 library. Require explicit consent before starting over; keep
  the existing E5 library and its indexes available rather than silently
  deleting or mixing them with Gemma results. Create a separate compatible
  SQLite/Zvec embedding space; include dimension and modality choices in its
  cache/index identity, and support interruption/retry during fresh indexing.
  Do not promise reuse of prior converted chunks or enrolled roots.
- Compare E5 and Gemma with the same labelled multilingual text and realistic
  code workloads; evaluate image retrieval and near-duplicate negatives
  separately, and audio/video retrieval if enabled. Report ranking quality,
  query latency, indexing throughput, peak RAM, disk/index size, and migration
  impact by dimension and supported CPU/GPU target. Do not promote an
  optional package until its signed artifacts, installed lifecycle,
  offline behavior, and release gates are qualified on supported platforms.
- Maintain HTTP/Tauri/mock client parity, accessible consent/settings and
  results, and regression coverage for model switches, prompt roles,
  dimensions, modality choices, stale evidence, failures, and scope isolation.

## Implementation Notes

- Extend `fm-semantic-components` model packs/catalog and
  `fm-semantic-worker` inference/storage boundaries; do not hand-edit
  generated API artifacts or introduce a separate component installer.
- Zvec is a derived fixed-dimension index; SQLite also stores cached vectors
  and chunk records. Dropping only the Zvec directory does not migrate the
  library. The existing ONNX mean-pooling backend accepts E5-style text
  graphs, not the upstream Gemma `safetensors` checkpoint.
- The Hugging Face checkpoint includes all encoders even when some are
  disabled at runtime. Measure actual optional-package and resident sizes
  before promising smaller modality-specific downloads.
- The comparison branch is research evidence, not a prerequisite branch for
  implementing this task. Record any new qualification results in the
  repository before changing the release default or enabling this option.

## Agent Notes

- 2026-10-07: Created after the E5/Gemma comparison. Product direction is a
  separate, opt-in Gemma installation alongside default E5, not a replacement
  or an assertion of superior text quality. Initialization locks dimensions
  and selected media types; changing them requires explicit reindexing.
- 2026-10-07: Implementation started. User chose to keep the worker pure Rust
  and qualify a native/exported inference runtime before exposing Gemma.
  Upstream ships a 1,488,915,288-byte safetensors checkpoint for a distinct
  `embedding_gemma2` architecture, not a graph usable by the current E5
  ONNX mean-pooling backend. Release/UI activation must remain gated until
  actual multimodal and cross-platform inference is verified.
- 2026-10-07: User confirmed this task must include native multimodal inference
  on supported CPUs, rather than shipping text/code first. As of this review,
  `lattice-inference` 0.11 has an Apache-2.0/MIT Rust CPU text encoder and
  optional Metal text path, but its EmbeddingGemma 2 loader explicitly ignores
  vision and audio tensors. `taconite-embeddinggemma2` handles image/text on an
  NPU, not the supported CPU targets. Neither qualifies an image/audio/video
  runtime; no Python sidecar or text-only option should be silently shipped.
  Qualify or build the missing CPU modality encoders and safe media
  preprocessing against upstream reference vectors first, then integrate
  signed model packaging, cross-platform tests, and product activation.
  Source: `ohdearquant/lattice` `crates/inference/src/model/embeddinggemma2.rs`
  and `Brishen/taconite` `taconite-embeddinggemma2/src/bin/embeddinggemma2.rs`.
- 2026-10-07: Added a failing-then-passing worker regression for dimension and
  token-limit cache collisions in `crates/fm-semantic-worker/src/embedding.rs`.
  The changed hash intentionally misses pre-existing cache rows on subsequent
  ingestion; existing indexed generations remain authoritative until rebuilt.
  The optional model, installation, media ingestion, UI, and platform
  qualification are not implemented yet; do not mark 0239 done.
- 2026-10-07: Worker unit tests (95), worker typecheck, and `pnpm run lint`
  passed; the full `pnpm test` passed its Rust and 2,332 frontend tests but
  failed in the unrelated `scripts/native-spa-smoke.test.mjs` fixture because
  its 15-second child timeout expired (reproduced in isolation). The cache
  change received a focused code review with no findings. Next work: first
  establish golden-vector parity and CPU resource/decoding bounds for text,
  vision, and audio on each supported platform; only then wire model packs,
  policy/migration, and frontend activation.
- 2026-10-07: First text/code Rust port is an opt-in `gemma-probe` feature in
  `fm-semantic-worker`, pinned to Lattice commit
  `17915f3748a4047c0af61bd8bc7066d1dccf24b4`. The local-only example and
  ignored real-checkpoint parity test cover search, question, code, and titled
  document prompts at 768/512/256/128 dimensions against generated upstream
  Python `SentenceTransformer` vectors (cosine >0.99999). The checkpoint
  fetch descriptor pins revision
  `914f7f89142e33e77833254d9c9b90c3cef7303b` and exact file hashes.
  No production path loads this crate or checkpoint.
- 2026-10-07: Direct PyTorch ONNX export of the **text-only** tower with
  masked-mean pooling and L2 normalization works after naming the graph output
  `sentence_vector` (the initial `embedding` output name collided with an
  internal Gather output). `scripts/probe-embeddinggemma-onnx.py --dynamic`
  exported a variable-sequence graph with approximately 1.0 GiB of external
  FP32 weights; ONNX Runtime CPU matched four pinned upstream prompt/dimension
  vectors to max absolute error <2.7e-7 at 15-21 tokens. Static export also
  matched. This demonstrates an ONNX text path, **not** an Optimum-supported
  automatic conversion or a drop-in E5 graph: export uses installed Python
  Torch/Transformers, and media encoders, long sequences, batching, platform
  parity, memory and throughput still require independent qualification.
  Text-only extraction excludes vision/audio weights and does not meet this
  task's multimodal CPU requirement. Keep 0239 in progress and Gemma hidden.
- 2026-10-07: Probe unit/parity tests, default worker tests, optional-feature
  clippy, repository lint, all 2,332 frontend tests, and 110/111 script tests
  passed. Full `pnpm test` stopped at a process-descendant cleanup test in
  `fm-semantic-docling`, which passed in isolation. The remaining script
  failure is the previously reproduced unrelated 15-second timeout in
  `scripts/native-spa-smoke.test.mjs`. These environmental failures are not
  evidence that the optional multimodal package is complete or qualified.
- 2026-10-07: User clarified that a text-only Rust/ONNX port has no product
  value: implement the **complete native Rust multimodal path** (including
  images, audio, and video) before offering Gemma; do not spend further
  implementation effort on ONNX unless every modality can be qualified. The
  checkpoint stores BF16 weights; the existing native text probe converts
  BF16 weights to FP32 and computes on CPU in FP32. The `f16` Lattice feature
  was unnecessary for BF16 decoding and has been removed. FP16 inference is
  forbidden; BF16 computation is an optional later optimization on verified
  hardware, not a default on CPUs.
- 2026-10-07: Started native multimodal inference at the checkpoint-backed
  projection seam in `gemma_multimodal.rs`: BF16/F32 weight validation and
  FP32 scale-free RMS normalization + learned projection from 768-dimensional
  vision or 1536-dimensional audio soft tokens into the 512-dimensional
  language-model space. Real-checkpoint Rust/Python goldens for both
  projections live in `embeddinggemma-multimodal-reference-v1.json`; FP16
  weight and malformed-input rejection are covered. This is one stage,
  **not** an image/audio/video embedding API. Next native stages: bounded
  image/video frame and 16-kHz audio preprocessing; Gemma4 vision tower
  (16 layers, patch/position encoding and pooling), Gemma4 audio tower
  (12 layers and subsampling), then a bidirectional language encoder that
  accepts projected soft tokens in place of placeholder IDs. The pinned
  Lattice text encoder only accepts token IDs, so its current `encode` API
  cannot simply be called after the projection. Compare each stage and
  end-to-end media vectors against Python before any worker integration,
  packaging, or UI activation. Video shares the vision tower; its processor
  samples up to 32 frames at 1 fps with 140 soft tokens per frame.
- 2026-10-07: After removing Lattice's optional `f16` feature, both native
  text and vision/audio projection goldens still passed using BF16 checkpoint
  weights decoded for FP32 CPU computation. Two projection unit tests, all
  100 worker unit tests, repository lint, and feature-gated clippy passed.
  Full `pnpm test` again stopped on the unrelated load-sensitive
  `fm-semantic-docling` descendant-cleanup test (passed immediately in
  isolation); remaining suites were not reached. This work is a first native
  inference stage and must not be described as support for media embeddings.
- 2026-10-07: Continued the **experimental, feature-gated** Rust CPU port:
  Gemma4 vision and audio towers, scale-free multimodal projection, and
  bidirectional soft-token language fusion now compose behind
  `GemmaNativeEncoder`. It supports model-owned text prompts and 128/256/512/768
  dimensions; bounded PNG/JPEG images; decoded 16-kHz PCM and bounded
  WAV/FLAC/MP3/M4A-AAC; and H.264 MP4/MOV video sampling with timestamps.
  Video uses 1 fps up to 32 seconds and uniform sampling across longer
  supported clips. Unsupported codecs, B-frame reorder, and decode/resource
  limit violations fail explicitly rather than returning a partial vector.
  CPU calculations are FP32 from BF16 weights; FP16 checkpoints are rejected.
  Python 5.19.0 goldens cover patch geometry and pixels, mel extraction,
  vision/audio tower outputs, fusion, full text roles, image, PCM, audio-file,
  and one-/two-frame video embeddings. The audio decoder's AAC route retains
  about 21 ms of priming in the tested M4A fixture.
  This remains a probe, **not** the installed optional package: signed
  artifacts, model/index identity and migration, consent/ingestion and host
  parity, cancellation and supported-target/GPU qualification remain open.
- 2026-10-07: Diagnosed the initial two-frame cosine shortfall to uint8 bicubic
  preprocessing, not language fusion. Torchvision rounds half values away
  from zero after each resampling pass; ties-to-even yielded 3,872 differing
  bytes in a 967,680-byte video frame and amplified through vision inference.
  Correcting the two passes reduced this to 32 bytes (one unit each) and
  projected-token maximum error to 0.014. Full image, one-frame video, and
  two-frame video cosines are now above 0.9999998 against pinned upstream
  FP32 vectors; the ignored parity test requires >0.99999 for all three.
  A bounded real H.264 MP4 also yields a vector and the expected presentation
  timestamps. The PCM16 WAV golden compares against Python inference on the
  *decoded quantized samples*: quantization alone shifts the unquantized
  vector cosine to about 0.999774, while native decoded-WAV parity against
  the equivalent upstream input exceeds 0.99999. These checks do not
  establish cross-platform support, retrieval quality, production throughput,
  or the optional installation contract.
- 2026-10-07: Confirmed the existing worker's index directory and activation
  marker key only model ID/revision, unlike its dimension-aware embedding
  cache. A dimension-only index change passed focused tests, but would silently
  force every existing E5 library to rebuild on upgrade. It was intentionally
  **not retained**: design the Gemma identity/migration together with a safe
  legacy E5 preservation path and explicit consent rather than changing this
  production path as a probe side effect.
- 2026-10-07: Added cooperative cancellation to the feature-gated native CPU
  text, image, audio, and video entry points and their inference stages.
  H.264 frame sampling, bicubic visual preprocessing, audio packet
  decoding/resampling, and mel feature extraction check cancellation during
  work rather than just before starting; stage errors map to one public
  cancellation variant. The earlier noncancellable probe APIs remain
  compatible. `fm-metadata` (35 library tests), `fm-semantic-worker` with `gemma-probe`
  (affected package tests), feature-gated Clippy, and ten ignored
  checkpoint-backed text/audio/image/video parity and cancellation tests
  passed. This does not implement an installed package or job wiring:
  signed Gemma artifacts, E5-preserving index/migration consent,
  policy-bound media ingestion, host/UI parity, and supported-platform
  qualification remain open; keep the option hidden and task in progress.
- 2026-10-07: User chose installation of the original pinned Hugging Face
  safetensors, tokenizer, and processor files directly, rather than creating
  another model pack or retaining a duplicate extracted checkpoint. The
  existing signed catalog, installer, and durable state assume exactly one
  payload per model; extend those contracts to verify and retain multiple
  immutable file artifacts as one atomic model installation, preserving the
  existing single-pack E5 path. Native inference must remain local/offline.
- 2026-10-07: Added signed original-file catalog entries and atomic multi-file
  installation, retaining E5's single-pack manifest and installation path.
  The component manager resolves all installed original paths only after
  checking each artifact's signed version, checksum, length, and safe file
  type; the native Gemma loader accepts the complete set of independently
  located files without repacking the checkpoint. Negative tests cover
  missing, unsafe, colliding, unreferenced, corrupt-download, and post-install
  tampered files. This is installation groundwork, not a Gemma production
  release: the current managed worker still takes an E5-style model pack and
  there is no signed Gemma catalog entry, model migration, media ingestion,
  UI activation, or supported-platform qualification. Original-file primary
  artifacts have a distinct signed kind; current desktop activation rejects
  them explicitly until native worker launch is wired. Keep the option hidden.
- 2026-10-07: User clarified that preserving or migrating an existing E5 index
  is not a prerequisite: users may initialize a fresh Gemma library. E5
  remains available and must not be silently deleted or mixed with Gemma.
  Continue through managed native launch and media ingestion before exposure.
- 2026-10-07: Added a creation-time `EmbeddingGemma 2` policy identity for
  128/256/512/768 dimensions and independent image/audio/video consent,
  persisted with a distinct embedding-space key for every combination.
  Existing E5 model identities remain unchanged; policy migration now rejects
  changing a Gemma library's model or media choices, requiring a fresh library
  instead. The host feed can admit only consented MIME types, with E5's
  document-only allow-list unchanged. A local release builder can stage the
  five hash-pinned upstream files as separate signed catalog artifacts.
  This does **not** make Gemma available to users yet: native managed-worker
  startup, actual media ingestion, new-library setup UI and host parity, and
  supported-platform qualification are still in progress.
- 2026-10-07: Integrated the optional original-file model with the managed
  native worker, a separate dimension/media-bound index, transactional
  fresh-library setup, consented media ingestion, typed search/Ask/code prompt
  roles, and desktop settings. An ignored optimized macOS test using the pinned
  checkpoint ingested PNG, MP3, and sampled H.264 MP4 through the real worker
  pipeline and retrieved each through text search, including video timestamp
  evidence; malformed video failed without publishing a result. On this local
  Apple Silicon machine, the three-media test at 128 dimensions took 44.94
  seconds wall time and reached 4,083,417,088 bytes maximum resident memory
  (`/usr/bin/time -l`, after compilation). This is one workload on one CPU,
  not a supported-platform resource ceiling. The unoptimized test
  took more than three minutes on its first image, so debug timings are not
  representative of a release build. E5 and Gemma index-switch tests preserve
  the prior E5 data. Without the `semantic-gemma` desktop feature, the desktop
  hides the optional profile and rejects direct setup/offer requests; an E5-only
  signed catalog has no Gemma profile. This is local macOS functional evidence,
  **not** supported-platform qualification: the existing release workflow does
  not yet fetch, build, sign, smoke, or measure the optional model across its
  four targets. Labelled retrieval quality, RAM and throughput by dimension,
  signed-artifact installation, and offline release lifecycle remain gates.
  Keep this task in progress and do not expose Gemma in the standard release.
- 2026-10-07: The local production bundle builder produced a Gemma-enabled
  release-mode worker plus all five verified original files alongside the E5
  model on macOS. Packaged-worker ingestion/restart, E5 model activation, Zvec
  recovery, and component acceptance smoke tests passed. The E5 production
  bundle smoke now also launches the **packaged** Gemma-enabled worker over
  authenticated IPC and ingests/retrieves PNG, MP3, and H.264 MP4 with
  timestamped video evidence offline (46 seconds in optimized mode on this
  macOS CPU). This does not verify a signed installed catalog or other targets.
  The E5 production
  evaluation's identity report now lists only its own runtime/model components;
  the catalog digest still binds all optional Gemma files. Its later
  `InvalidObservation("multilingual-recall")` failure also reproduces with an
  **E5-only control bundle**: the evaluator compares Ask's fused candidate
  scores and chunks with a separate dense-search result and a dense-score
  threshold. Do not weaken its release gate or treat the local bundle as
  qualified; correct that evaluation contract separately before publishing.
- 2026-10-07: Repaired the E5 evaluator's Ask-versus-dense score-domain
  contract. Reports retain independently validated raw dense candidates and
  their unchanged 0.84/0.02 floor, and now record the actual Ask candidates
  plus an explicit dense-similarity or hybrid-reciprocal-rank domain. Ranked
  context, authorization, citation, and retrieval metric checks still apply;
  hybrid ranks are verified rather than compared to cosine floors. Older
  approved reports without the new optional fields remain readable and use
  the previous dense validation. Both the E5-only control bundle and the
  Gemma-enabled local macOS bundle completed packaged smoke and produced
  blocked evaluation reports (missing other supported targets and manual
  release evidence). The Gemma bundle's E5 evaluation measures E5, not
  Gemma retrieval quality. Cross-platform and independently labelled Gemma
  comparisons, installed signed-catalog lifecycle, and release feature
  qualification remain open; keep this option hidden in standard releases.
- 2026-10-08: Added a development-only, locally signed Gemma catalog alongside
  the existing E5 options. `pnpm dev:tauri:semantic:gemma` verifies pinned
  upstream files, builds an optimized native worker, and exposes the existing
  immutable setup/installation and folder-enrolment UI in a Tauri debug host.
  The developer host resolves installed original files and library policy at
  each worker launch. A local isolated install and native query succeeded;
  testing a representative user-selected folder is the purpose of this opt-in
  build, not a production quality or cross-platform qualification result.
- 2026-10-08: Fixed development installation beside an active E5 embedding space:
  signed Gemma artifacts are staged before atomically activating the separately
  initialized empty library, without deleting E5's package or library. Direct
  installation requires a matching Gemma library policy, and a new signed offer
  can retry after a failed attempt. Settings now loads Gemma's offer on selection,
  keeps dimensions and media consent in the primary view, and collapses the
  artifact inventory into optional technical details. An isolated macOS
  developer-bundle test installed E5 followed by signed Gemma originals and
  launched a native Gemma worker query; representative folder quality and
  other platform qualification remain open.
- 2026-10-08: Corrected component status projection for the signed Gemma
  original-file artifact: an activated `OriginalModel` must count as the
  active model, just like a packaged `Model`. Otherwise the desktop reported
  semantic-library enrolment unavailable even though installation and native
  worker launch succeeded. The signed development bundle now verifies the
  active model and enrollable Gemma library after installing beside E5.
- 2026-10-09: Knowledge retrieval now selects Gemma's SearchQuery,
  QuestionAnswering, or CodeRetrieval prompt per explicit query intent, instead
  of using QuestionAnswering for every search. The Knowledge pane defaults to
  Documents, offers Questions and Code beside the query, and selects Questions
  when Ask starts without existing results. Ask over displayed evidence still
  does not rerun retrieval. Needs-based query expansions remain available in
  Advanced rather than occupying a second toolbar row; the DSL editor keeps
  the selected prompt mode unchanged. E5 continues using its existing query
  prefix regardless of intent. This does not qualify Gemma for release.
- 2026-10-09: The private four-target semantic-component qualification workflow
  can now explicitly fetch verified, pinned Gemma originals and build/smoke
  Gemma-enabled worker candidates without changing its default E5-only path.
  Publication still rejects catalogs containing Gemma until independent
  model-specific quality and supported-platform evidence is reviewed and a
  release decision is implemented. This does not enable Gemma in standard
  desktop builds or change the workspace version to 0.5.0.
- 2026-10-09: Read-only qualification run 37971224110 on main
  (96d697ad4d2bf4adcb546617102831ec290f58b8) built the four Gemma
  candidates. Windows passed packaged smoke, lifecycle/privacy qualification,
  and payload upload. Both Linux targets and macOS failed the packaged Gemma
  ingestion smoke at its 60-second per-job test deadline; therefore catalog
  signing and aggregate collection were skipped and no release was published.
  The Gemma-specific smoke deadline is now 300 seconds, with per-media elapsed
  times logged, while the E5 deadline stays at 60 seconds. The initial
  Windows result and E5 evaluation alone do not qualify Gemma.
- 2026-10-09: Read-only rerun 37974984924 on the bounded-smoke commit
  d3f921fe08e4994a905727a71af102979f4fbe23 succeeded on all four
  supported targets: packaged smoke, lifecycle/privacy checks, signed
  per-target catalog, aggregate E5 evaluation, and fingerprint-locked
  candidate collection. The retained candidate artifact is
  `semantic-component-candidate-d3f921fe08e4994a905727a71af102979f4fbe23`
  (ID 11640886748; expires 2026-10-23). The Linux ARM signed catalog contains
  both E5 and the pinned Gemma original-file profile (`embedding-gemma2`).
  Publication was skipped. This qualifies the candidate workflow path, not
  Gemma for product release: independently labelled model-specific retrieval
  quality, per-dimension/platform latency and peak RAM, signed installed
  offline lifecycle and release decision remain open.
- 2026-10-09: A separate [native CPU retrieval probe](../docs/evaluations/embeddinggemma-native-qualification.md)
  uses the candidate's hash-verified originals and the same labelled
  multilingual and generated-code fixtures as the exploratory E5 comparison.
  This macOS M4 Max run measured knowledge MRR 0.95 (E5's earlier Python
  baseline was 1.0) and code MRR 1.0 at all four Gemma dimensions. Four
  synthetic text-to-image queries each ranked their labelled colored shape
  above same-color or same-shape distractors. The mixed run peaked at
  2,583,347,200 bytes RSS; image throughput was 0.039/s. The probe records
  per-case misses and unanswerable queries without pretending to qualify
  rejection. Broader independent labels, photographic/media negatives,
  platform/dimension resource data, matched packaged E5 comparison and
  installed/offline lifecycle remain open; Gemma stays hidden.
- 2026-10-09: **Signed installed lifecycle, macOS ARM only.** Re-downloaded the
  exact read-only candidate from run
  [37974984924](https://github.com/erikvullings/procyon/actions/runs/37974984924),
  artifact `semantic-component-candidate-d3f921fe08e4994a905727a71af102979f4fbe23`
  (ID **11640886748**, retained until 2026-10-23). The macOS-aarch64
  signed catalog revision was
  `procyon-macos-aarch64-0.4.2-417f9e72bdc56f325f336878e70b1cf0`;
  catalog SHA-256
  `e81c2d43ec2cc3a8bc1146d3fcca5d0f94d465e9f97e9af4bdf78d982cf660d3`,
  detached-signature SHA-256
  `1c238c6a87f5a272e19401e7c00162be4703ca47cd78a92881ed5f60a7deee67`.
  A locally staged hard-linked subset contained exactly the eight catalog
  payloads, without changing their bytes; the public environment verification
  key authenticated the catalog and the harness checked all signed payload
  hashes. An opt-in `PROCYON_GEMMA_INSTALLED_TEST` mode of the existing
  `qualify_semantic_lifecycle` example installed the Gemma profile through
  its filesystem-only source and explicit installation offer/consent. Its
  fresh manager verified all seven *selected* installed artifacts (worker,
  Zvec runtime, and five originals); the catalog's E5 pack was verified as
  a downloaded payload but not installed. A copy of the verified installed
  worker was staged beside a copy of the verified runtime, as in the desktop
  launcher, while the worker used the installed originals. The ignored
  packaged-worker test ingested and retrieved PNG, MP3, and H.264 MP4 offline
  (27.6/2.1/12.4 seconds), including sampled-video timestamps; after killing
  the worker, a new worker recovered all three document indexes. The full
  lifecycle harness passed absent-state, low-disk, signature tampering,
  payload corruption, interrupted/resumed install, manager restart,
  installed-worker tampering, cross-process locking, explicit retain/delete
  uninstall, and clean reinstall checks with **zero network artifact reads**.
  The successful local report is `target/gemma-installed-report.json`
  (task-local, not a published release artifact). An initial run against the
  combined four-target artifact correctly rejected unrelated files, and a
  second attempt exposed macOS's `@loader_path` requirement; both were fixed
  in the test setup before the clean passing run. This checks neither the
  desktop's durable library-consent UI nor cross-platform installed lifecycle:
  Linux x86_64/aarch64 and Windows x86_64 were **not tested** by this gate.
  Preceding-candidate upgrade/rollback remains blocked without an exact
  preceding signed candidate; native assistive-technology and packaged UI
  checks still require operators. Independent realistic retrieval labels,
  matched E5 comparison, and per-dimension/platform resource limits remain
  open. Keep the release guard and Gemma's default-hidden status intact.
- 2026-10-09: **Three remaining signed installed targets passed** in read-only
  [run 37990723942](https://github.com/erikvullings/procyon/actions/runs/37990723942)
  (qualification harness SHA `9efb7c8947f3ab201b197cdc1e151c1cdac5b54c`).
  The manual workflow downloaded the *same* retained candidate from run
  37974984924, artifact ID 11640886748, and skipped all payload-building,
  catalog-signing, collection, and publication jobs. Each target verified
  its detached catalog signature with the public environment key and all
  exact signed payload hashes before testing an offline installation. The
  existing lifecycle harness installed the Gemma worker, runtimes, and five
  original files through an explicit offer/consent, reverified installed
  bytes after manager restart, ingested and queried PNG/MP3/H.264 MP4 with
  timestamp provenance, killed/restarted the worker and recovered all three
  indexed documents, then checked tampering, interrupted resume,
  cross-process locking, retain/delete uninstall, and clean reinstall.
  Each uploaded report records **13 passed checks and zero network artifact
  reads** (the earlier Actions artifact download is not an offline operation):

  | Target | Installed report artifact ID | Catalog SHA-256 | Signature SHA-256 |
  | --- | ---: | --- | --- |
  | Linux ARM64 | 11645322065 | `b158c0b7c2f25aa16215fdacc34268d2326e1c26fb0f593a09617bd987e534df` | `4e25397654c2c47afd60920501ca13731d0617dc397482ce395062cf11dec34e` |
  | Linux x86_64 | 11644539558 | `c7ae0e2625d9d83a6e93621a6bb51fd2731200ede4cd188dd8b389b58d8f1724` | `5ce2a65b0376594ff58037597125d46ae104c36569c27315e0f6c90aa043d0ad` |
  | Windows x86_64 | 11645457780 | `83cb7c83d0f279ab85c97c127ba87969046d0ef6f65c39e65d973fab127fc382` | `fa890ce8cb0e39ce05fc5e7c64110084da925f73c22d0a0df9f7c1993496516b` |

  The first x86 attempt (run 37986807776) failed *before installation*
  because the CI-built test binary lacked `ORT_LIB_PATH`; the diagnostic
  retry (37988792465) confirmed an unresolved `OrtGetApiBase` linker symbol
  and was stopped after the diagnosis. A further run (37989503641) linked
  against the retained signed ONNX payload, then failed at test startup
  because its shared library was not on the test process loader path.
  The passing run staged that **verified installed** ONNX payload beside the
  installed worker and Zvec library, rather than using an unsigned/system
  replacement. The macOS ARM result above plus these three reports establish
  this bounded signed installed/offline gate on all four supported targets,
  **not** Gemma release qualification. All four reports still mark
  preceding-candidate upgrade/rollback blocked without an exact prior signed
  candidate; native screen-reader and packaged consent/progress/error UI
  checks require operators. Independent realistic multilingual/code/media
  labels, matched E5 quality, and dimension/platform CPU resource bounds are
  still missing. Do not enable Gemma or lift the publication guard.
- 2026-10-09: The bounded [four-target signed CPU resource probe](../docs/evaluations/embeddinggemma-signed-cpu-2026-10-09.md)
  passed in read-only run
  [37992680322](https://github.com/erikvullings/procyon/actions/runs/37992680322)
  using the exact retained candidate artifact 11640886748, with all
  build/sign/collect/publish jobs skipped. Installed originals powered a
  fresh native worker/index at **128, 256, 512, and 768 dimensions** on
  macOS ARM, Windows x86, Linux x86, and Linux ARM. Each dimension ingested
  one synthetic PNG, MP3, H.264 MP4, and short text document; three text
  queries per modality, worker restart/recovery, bounded wall time and
  per-item ingestion rates are recorded in the checked-in machine reports.
  Across dimensions, OS worker RSS high-water values were 3.46 GiB on
  Linux ARM, 3.47 GiB on Linux x86, and 3.42 GiB on Windows. The macOS
  virtual M1 runner's **sampled** maximum was at least 3.33 GiB; it is
  not an exact peak. Linux ARM exposed only `unknown` for the CPU model,
  but the runner image, CPU count and RAM are recorded. One synthetic
  asset per modality is neither sustained throughput nor a representative
  resource ceiling. Realistic independently labelled retrieval quality,
  larger and hard-negative media, matched E5 measurements, prior signed
  candidate upgrade/rollback, and manual accessibility/consent UI review
  remain open. Do not promote Gemma or change the release guard.
- 2026-10-09: The [bounded independently sourced quality probe](../docs/evaluations/embeddinggemma-labelled-2026-10-09.md)
  authenticated the exact retained candidate 11640886748 and compared its
  Gemma native FP32 CPU encoder (128/256/512/768d) with its E5 ONNX CPU model
  (384d) on **identical** source-pinned corpus/query fixtures on one macOS ARM
  host. Mr. TyDi v1.1 supplied 16 human-labelled Swahili/Telugu dev questions;
  their lexical distractors remain *unjudged* and must not be claimed as hard
  negatives. Four CodeSearchNet Python queries used seven positive and ten
  explicitly grade-0 code snippets from commit-pinned, licence-checked repos,
  excluding contradictory/ambiguous annotations. Five SugarCrepe swapped
  caption pairs used real COCO photos whose **individual current** Flickr
  CC BY/BY-SA licences were checked (photos not redistributed); E5 has no
  image encoder. Gemma's text hit@1 was 8/8 in each language; E5 was 7/8 in
  Swahili and 8/8 in Telugu, but E5's one top distractor is *unjudged* and
  may be relevant. E5 hit@1 was 4/4 on code; Gemma hit@1 was 3/4 at **all**
  dimensions, ranking human-irrelevant snippets ahead of a relevant buffered
  file reader. Gemma preferred the wrong hard caption for one of the five
  real photos at **all** dimensions. Per-case machine reports and the
  reproducible harness/source digests are retained beside that document.
  This deliberately small one-host model/runtime comparison is **not**
  release qualification: a larger explicitly judged multilingual
  hard-negative pool, broader code/photo coverage including photo-to-photo
  negatives, lawful media labels, and installed-target matched quality
  remain open. Preserve the release guard and default-hidden Gemma.
- 2026-10-10: The [larger human-judged retrieval slice](../docs/evaluations/embeddinggemma-judged-2026-10-10.md)
  replaced unjudged multilingual distractors with **explicit grade-0,
  native-speaker-annotated MIRACL negatives**: 50 Swahili and 50 Bengali dev
  questions, one judged positive and three pooled/judged hard negatives each,
  from pinned ~70 MiB source archives. It expanded realistic CodeSearchNet
  code from four to **14** human-graded queries, 48 snippets from
  47 exact-revision permissively licensed repository sources; no external
  passage, code, or photo bytes were checked in. The exact signed candidate
  Gemma native CPU at 128/256/512/768d and E5 ONNX at 384d ranked identical
  inputs locally on macOS ARM. At 768d Swahili hit@1 was **.86 Gemma / .72
  E5** and Bengali **.80 / .66** over four judged candidates/query. Paired
  per-query wins/ties/losses were 8/41/1 and 11/35/4; conditional bootstrap
  intervals for Gemma-minus-E5 were [+.04,+.26] and [.00,+.28] (not
  population-wide confidence). Code hit@1 was **.857/.857** at 768d, with
  wide paired interval [-.214,+.214]; at 128d Gemma fell to .714. The
  buffered-reader code failure persisted at every Gemma dimension despite
  production-matching prompt roles and no token truncation of decisive
  snippets. Its highest-ranked grade-0 distractor has only one annotator,
  so attribution to model quality versus judgement/document context remains
  uncertain. All per-query failures and the exact pinned-source hashes are
  retained in machine reports. These deterministic four-candidate pools,
  one-host model/runtime confound, Wikipedia source-rights caveat, sparse
  code labels, untested installed-target quality, and still-small 4/5 photo
  evidence **do not qualify Gemma for release**. Do not enable, publish or
  change the guard.
- 2026-10-10: The [bounded real-photo media follow-up](../docs/evaluations/embeddinggemma-media-2026-10-10.md)
  added five distinct, individually live Flickr CC BY/BY-SA-verified COCO
  photographs with human-validated SugarCrepe hard-negative captions (ten
  photos total, three pinned annotation categories). Exact signed candidate
  11640886748 ran natively on macOS ARM at 128/256/512/768 dimensions:
  **8/10** correct caption preferences at each dimension. The prior
  teddy-bear/sheep failure and a new kitchen attribute swap both ranked
  their negative higher at every dimension. The committed per-pair report
  and source/image hashes permit replay, but this opportunistic tiny sample
  is not representative image retrieval or E5-matched evidence. No
  independently judged, rights-verified photo-to-photo hard negatives
  were obtained. Winoground Getty images returned HTTP 401 without accepting
  gated terms; Clotho's positive audio captions lack judged hard negatives
  (and require individual source-rights checks), while examined Charades-STA
  temporal annotations lack judged negative intervals and verified clip
  rights. No audio/video results were fabricated. These are still open
  quality/release gates; do not enable, publish or change the guard.
- 2026-10-10: Release owner authorized an **explicitly opt-in
  EXPERIMENTAL** signed package so users can evaluate Gemma in practice
  despite unresolved quality gates. The
  [release decision](../docs/evaluations/embeddinggemma-experimental-release-2026-10-10.md)
  records truthful E5-vs-Gemma and media limitations, 8 GiB model RAM
  planning estimate, large download, CPU costs, and immutable per-library
  dimensions/media choices. A separate reviewed experimental lock binds
  the exact four-target candidate run 37974984924, source, release tag,
  aggregate E5 negative-control report, catalogs and signed asset bytes.
  The existing default release gate still rejects Gemma; a manual
  publication override requires independent Gemma-specific approval and
  signature verification. No artifact was published, no release was tagged,
  and standard desktop builds are not enabled; concurrent UI/consent and
  Metal/fallback integration must be combined and checked first. Task
  remains in progress.
- 2026-10-10: Integration correction: the reviewed candidate
  37974984924 predates the automatic Metal worker and mixed-media UI.
  Its experimental approval lock is now explicitly **blocked** (`noGo`),
  so environment gates alone cannot publish it. A new signed four-target
  candidate from the integrated source, matched release-tag URLs, and a
  new reviewed experimental approval are mandatory. Standalone media
  enrollment from authorized folders must also be verified end-to-end
  before claiming mixed-media search in an experimental release.
- 2026-10-10: An opt-in macOS ARM application integration probe of the
  **old signed CPU worker** confirmed provider-enumerated, consented PNG,
  MP3 and MP4 files are read through VFS and ingested over IPC; tenant
  search returned all three and host citation resolution recovered their
  enrolled source locations, while another tenant received no results.
  This checks the host media path (not just direct worker ingestion) but
  uses synthetic files on one host; it does not qualify the still-unbuilt
  integrated automatic-Metal candidate, UI, or retrieval ranking quality.
- 2026-10-09: Added a separate, explicitly selected `gemma-metal` image probe on
  macOS using the pinned Lattice FP32 Metal GEMM primitive in the native vision
  tower, vision projection, and soft-token language fusion. Default/managed
  inference stays CPU; FP16 is not used. A pinned-checkpoint M4 Max test executes
  620 Metal GEMM dispatches per image and matches CPU/upstream image vectors at
  all 128/256/512/768 widths (cosine >0.99999). The measured direct test process
  peaked at 2,521,169,920 resident bytes. End-to-end encode times remain about
  28-31 seconds for CPU and 29-30 seconds for Metal on one patterned image,
  with a 50.7-second Metal outlier on a repeat: **no acceleration is qualified**.
  See `docs/evaluations/embeddinggemma-metal-image.md` for method and numbers.
  Metal remains hidden; a device-resident vision attention path, supported-target
  measurements, and fallback qualification are still required before exposure.
- 2026-10-09: Repeated the exact optimized, pinned-checkpoint image benchmark
  twice after the user's 68 GB MLX-Serve model was unloaded. At every width,
  Metal remained 0.57-1.82 seconds slower per image than CPU; both runs
  retained >0.99999 reference parity and 620 GPU GEMM dispatches, with
  2.48-2.49 GB test-process peak resident memory. The prior 50.7-second
  Metal outlier did not recur. A separate small `omp --model` process
  remained, so these are not guaranteed GPU-exclusive measurements.
  See `docs/evaluations/embeddinggemma-metal-image.md`; do not promote Metal
  until stage-level profiling and a demonstrable benefit justify it.
- 2026-10-09: Profiled that GEMM-only image probe and found vision attention
  consumed 25.96-26.60 seconds on CPU, versus ~1.1 seconds spent in 620
  synchronous Metal GEMM calls. Added FP32 Metal query/key and
  probability/value GEMM in each vision head, retaining CPU softmax and the
  CPU default. Pinned-checkpoint final release-mode image parity passed at
  all 128/256/512/768 widths: Metal 5.64-5.83 seconds versus CPU 27.83-28.39
  seconds for one patterned PNG, with 1,004 actual Metal dispatches and
  2,523,824,128 bytes direct-process peak resident memory. This is a measured
  ~4.8-5.0x local M4 Max speedup, not installed-worker or cross-platform
  qualification. Keep Gemma Metal hidden until supported-target resource,
  quality, and fallback gates have independent evidence.
- 2026-10-10: Integrated automatic Metal image selection from
  `gemma-metal-images` into the 0.5.0 experimental release source. Only
  newly qualified macOS ARM Gemma candidate workers compile `gemma-metal`;
  other targets and ordinary E5 candidates remain CPU-only. The historical
  four-target Gemma candidate is explicitly no-go and cannot be reused.
  A fresh four-target signed run, installed-worker checks, integrated
  application/UI media verification, and an exact reviewed approval are
  still required before publishing or tagging a desktop release.
- 2026-10-10: Integrated signed candidate run 38077959991 at source
  `c6c7a94058950bf2704aca484f9c4b77c6dd2dcf` passed four-target
  catalog/payload verification, and exact installed run 38082235196 passed
  13 offline automated lifecycle checks per target (including media and text
  at all four dimensions); both runs skipped publication. The new signed
  macOS worker passed host/VFS/IPC authorized mixed-media retrieval with CPU
  fallback and selected FP32 Metal automatically for an image-only library.
  The installed reports retain blocked preceding-candidate upgrade/rollback
  and manual-required accessibility and packaged UI checks. The candidate
  evaluation and experimental lock remain `noGo`; no release-owner approval,
  tag, component publication, or desktop release has occurred. See the
  [release decision](../docs/evaluations/embeddinggemma-experimental-release-2026-10-10.md)
  for the exact retained artifact and expiry. Task remains in progress.
- 2026-10-10: The existing experimental lock now binds the **new** signed
  candidate's run/source, manifest and evaluation hashes, original model,
  four catalogs and signatures; its decision stays `noGo` with
  `blocked-pending-release-owner-review`. On macOS, the signed-worker
  application regression additionally confirmed deletion revokes image
  citation and file-primary display after reconciliation, and shuts down
  cleanly before temporary index cleanup. Mock-client UI tests cover consent,
  install progress/error states, deletion choices, media navigation, stale
  results, and labelled/focusable controls, but not a real packaged native
  UI or VoiceOver. The unpublished catalog and no-go gate preclude honest
  live desktop installation preflight until explicit approval. Manual checks
  and release-owner review remain outstanding.
- 2026-10-10: A bounded isolated macOS ARM Tauri **development** UI preflight
  used the locally development-signed `darwin-arm64-gemma-metal` bundle, not
  the unpublished production-signed candidate. Native accessibility inspection
  found labelled Gemma consent/media/dimension controls; explicit 768d
  image/audio/video consent enabled installation, which displayed progress and
  reached "Installed and enabled". Keyboard Return opened an isolated media
  folder, and folder inclusion was confirmed with indexing generation 1.
  Its catalog nevertheless contained zero documents, so native media results,
  source opening, stale/unavailable behavior, and deletion were **not**
  exercised. The fixtures sat under ignored `target/`; this is not proof of
  a general indexing defect. No deliberate install-error or VoiceOver session
  or Windows/Linux native UI review was performed. See the release decision
  document for the exact observations and remaining gates; keep `noGo`.
