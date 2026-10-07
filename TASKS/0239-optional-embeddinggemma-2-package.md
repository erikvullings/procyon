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
- Switching from E5 to Gemma stages the installed package, obtains explicit
  reindex consent, preserves enrolled roots and policy, rebuilds a separate
  compatible SQLite/Zvec embedding space, and supports interruption/retry
  without mixed-model results. Include dimension and other
  embedding-affecting settings in cache/index identity; the existing cache
  key does not include dimension. Reuse retained, authorized converted chunks
  where safe, but fall back to reconversion when absent or incompatible;
  never assume excluded or unbacked-up chunks are recoverable.
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
