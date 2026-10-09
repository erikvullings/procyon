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
