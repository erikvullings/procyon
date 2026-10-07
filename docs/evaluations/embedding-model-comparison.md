# EmbeddingGemma 2 versus multilingual-E5-small (2026-10-07)

**Decision: do not migrate yet.** On this small, repository-owned retrieval corpus,
EmbeddingGemma 2 did not improve ranking over E5, and its text-only CPU inference
was substantially slower and larger. This is an exploratory *model-level* comparison,
not a production-equivalent release qualification or evidence that either model
generalizes to larger private libraries. Keep the existing E5 production path.
Reconsider only after larger, independently labelled multilingual and multimodal
corpora, measured packaged/offline inference on supported platforms, migration
recovery testing, and a user-visible resource budget are available.

## Reproduce

The runner is [`scripts/compare-embedding-models.py`](../../scripts/compare-embedding-models.py).
It uses the existing repository-owned
[`knowledge-retrieval-corpus-v1.json`](../../crates/fm-application/tests/fixtures/knowledge-retrieval-corpus-v1.json),
not user documents. It includes 30 eligible chunks, 10 positive queries and 3
negative controls. Both models see the same section-path-plus-chunk text and query
strings. E5 uses `query: ` / `passage: ` and Unicode case folding, matching the
current worker; Gemma uses its `SearchQuery` / `Document` prompts and no title.
Both return L2-normalized vectors. Gemma's full vectors are sliced to
128/256/512/768 dimensions **and renormalized** for both queries and documents.
The exact same ranking and binary file/chunk recall@10, MRR, and nDCG@10
calculations apply to both. Negative controls are excluded from ranking metrics
because there is no comparable calibrated acceptance threshold; they are **not**
claimed as correctly rejected. No Zvec, hybrid retrieval, answer generation,
reranking, source lifecycle, or scope-authorization implementation is exercised.

Python 3.11, the pinned transitive package snapshot
[`embedding-model-eval-requirements.txt`](../../scripts/embedding-model-eval-requirements.txt),
and the two immutable Hugging Face revisions below are required. On the tested
Mac, `uv venv /tmp/procyon-embedding-eval --python 3.11` followed by
`uv pip install --python /tmp/procyon-embedding-eval/bin/python -r scripts/embedding-model-eval-requirements.txt`
installs the evaluation tooling; the lock snapshot is **not** a production
dependency set or a cross-platform binary guarantee. Authorize the gated Google
model on Hugging Face if necessary. Run each model in a separate process, with
`HF_HOME` set to a local cache (and `HF_HUB_OFFLINE=1` after downloading):

```sh
python scripts/compare-embedding-models.py e5 --output /tmp/e5.json
python scripts/compare-embedding-models.py gemma --output /tmp/gemma.json
python scripts/compare-embedding-models.py gemma --image-smoke --output /tmp/image.json
python scripts/compare-embedding-models.py gemma --audio-smoke --output /tmp/audio.json
python -m unittest discover -s scripts -p 'test_compare_embedding_models.py'
```

Here `python` is the evaluation venv interpreter. Reports contain per-case ranked
chunk IDs; keep reports local rather than committing private-workload results.
Model revisions: E5 `614241f622f53c4eeff9890bdc4f31cfecc418b3`;
Gemma `914f7f89142e33e77833254d9c9b90c3cef7303b`. The corpus SHA-256
in both local reports was
`c4591389db16b6ba91dba1b4795f9a903ae038b27dccc7aeba852c497f502abb`.
The run used macOS 27 arm64, 128 GiB RAM, CPU FP32, four PyTorch threads,
PyTorch 2.14.1, Transformers 5.19.0, Sentence Transformers 6.1.0,
NumPy 1.26.4. It makes no telemetry calls, but the initial model download
contacts Hugging Face.

## Measured results

| Model | Dimensions | File recall@10 | Chunk recall@10 | MRR | nDCG@10 |
| --- | ---: | ---: | ---: | ---: | ---: |
| E5 | 384 | 1.000 | 1.000 | 1.000 | 0.988 |
| Gemma | 128 | 1.000 | 1.000 | 0.950 | 0.961 |
| Gemma | 256 | 1.000 | 1.000 | 0.950 | 0.961 |
| Gemma | 512 | 1.000 | 1.000 | 0.950 | 0.969 |
| Gemma | 768 | 1.000 | 1.000 | 0.950 | 0.969 |

One `match-sorting-regression` case lost top-file rank for Gemma; all ten
positive cases saturate recall@10 because the corpus has only 30 chunks. Do
not interpret the small difference as statistically meaningful model superiority.

| CPU FP32, separate processes | E5 384d | Gemma text 768d |
| --- | ---: | ---: |
| Model construction from cached files (seconds) | 0.91 | 1.57 |
| 30 documents, batch size 8 (documents/second) | 297 | 51 |
| 13 one-at-a-time queries, median / p95 (ms) | 5.14 / 6.44 | 24.52 / 32.78 |
| Process peak RSS, including Python/runtime (GB) | 1.03 | 2.40 |
| Downloaded checkpoint `model.safetensors` (bytes) | 470,641,600 | 1,488,915,288 |

Construction times are **warm OS-cache** timings, exclude Python startup and
download, and are not comparable cold-start guarantees; a cold disk-load
measurement was not obtained. CPU latency is a
small-batch Python/PyTorch measurement, **not** the shipping E5 ONNX worker;
there is no validated Gemma ONNX equivalent here. The pinned packaged E5
model is 487,353,895 bytes including tokenizer and config, and its ONNX weight
file is 470,268,510 bytes. Gemma's omitted vision/audio encoders save runtime
memory but this Hugging Face checkpoint still downloads the full ~1.49 GB
weight file. Neither model's index or auxiliary runtime files are in the
checkpoint-size row.

At FP32, raw vectors for one million chunks occupy approximately 0.512, 1.024,
2.048, or 3.072 GB at Gemma 128/256/512/768d respectively; E5 384d uses
1.536 GB. This excludes SQLite/Zvec metadata, HNSW, multiple generations,
WAL, and migration overlap. 128d on the same Google model card drops
multimodal benchmark quality noticeably, so no default dimension is
established by this small text fixture.

## Media feasibility and boundaries

- **Images:** Generated geometric 256x256 original, resized/recompressed
  JPEG and unrelated triangle. Gemma text+vision FP32 peak RSS 3.08 GB;
  three image embeddings took 2.63 seconds including first-inference setup.
  Original-to-duplicate cosine 0.987 versus unrelated 0.787; the duplicate
  ranked first. One example does **not** establish a duplicate threshold,
  recall on photographs, crop/rotation robustness, or replace task 0170's
  opt-in perceptual-fingerprint approach.
- **Audio:** A generated one-second 440 Hz mono 16 kHz WAV yielded a finite,
  normalized 768d vector in 2.90 seconds including first inference. Gemma
  text+audio peak RSS 3.91 GB. No meaningful semantic sound retrieval,
  transcription, language, or long-input evaluation was run.
- **Video:** The model card documents frame sampling at 1 FPS with the vision
  encoder and an 8,192-token shared input budget. No successful end-to-end
  video decode/embedding result was produced: the tested Sentence Transformers
  / Transformers stack requested TorchCodec, whose FFmpeg 4–8 binaries could
  not load this host's FFmpeg 9 shared libraries. Installing the published
  video extra and TorchCodec 0.12.0 did not resolve that native mismatch.
  This is a packaging blocker to validate on each target, **not** evidence of
  model incapability. Image and video both require the vision encoder.

## Migration and offline packaging

`fm-semantic-conversion/src/chunk.rs` fingerprints structural embedding input
independently of the embedding model; `fm-semantic-worker/src/semantic_storage.rs`
stores published chunk text, section path, excerpt, provenance, occurrence
and generation in SQLite `records`, with vectors in reference-counted
`vectors`. **Existing published, authorized chunk text can in principle be
retained and re-embedded without re-reading or re-converting each source,**
provided the converter/chunker contract, completeness, and source scope are
checked. But there is no durable model-independent, independently restorable
chunk archive: ordinary backup excludes rebuildable chunks; exclusions and
retention remove them; `records.cache_key` has a foreign key to the vector.
An interrupted or offline migration cannot assume missing chunks can be
recovered. The existing ingestion path converts before embedding and does not
implement this chunk-only migration.

`LibraryIndexManifest` rejects dimension, model revision, tokenizer,
preprocessing, normalization, converter, or chunker mismatch. The embedding
cache key incorporates model identity and chunker; E5 vectors cannot be reused
in Gemma's space. **Dimension is not currently included in
`EmbeddingCacheKey::calculate`**, despite the globally shared `vectors`
table storing a dimension. Two libraries using the *same Gemma revision*
at different truncation dimensions could address the same cache key; a
multi-dimension implementation must key by dimension and validate cache hits
before indexing. Migrating needs an authorized, resumable shadow generation
or catalog/index, fresh vectors for all surviving records, reference and
scope checks, an atomic publication/rollback boundary, and space for both
old and new vectors/indexes. For **the same Gemma revision**, shorter vectors
can be derived from retained 768d vectors by prefix truncation plus L2
normalization; increasing dimension from a stored 128/256/512d vector
requires re-embedding. Today's fixed-dimension index does not support either
operation in place. Conservatively, **any post-initialization dimension
change requires a full vector re-index** while reusing checked chunks where
possible; never merely relabel the index dimension.

For a possible future implementation, the user selects exactly one
128/256/512/768 dimension at library initialization, and separate
image/audio/video opt-in checkboxes become immutable after initialization.
Video needs the vision encoder even if image indexing is unchecked. Google's
compatible encoder spaces technically allow enabling another encoder without
re-embedding existing text, but the proposed immutable choice is a product
constraint, not an assertion of technical necessity. Keep task 0170's exact
checksum and perceptual-similarity UX distinct from semantic media search.
No initialization settings or production ingest paths were changed here.

The shipping E5 path has an immutable, SHA-verified model package and
offline ONNX CPU worker on supported targets. Gemma here was loaded from
Hugging Face with Python/PyTorch and a gated checkpoint, not packaged by
the signed semantic component catalog, tested offline in the product,
converted to a validated native CPU runtime, or tested on Linux/Windows.
Gemma requires FP32 on typical CPUs (or BF16 with native support); **FP16
is unsafe** due to possible non-finite/silent quality degradation. Size and
licensing/compliance of redistributed encoder subsets still need review.

Sources: [Google developer guide](https://developers.googleblog.com/embeddinggemma-2-the-developer-guide/),
[EmbeddingGemma 2 model card](https://huggingface.co/google/embeddinggemma-2),
[multilingual-E5-small model card](https://huggingface.co/intfloat/multilingual-e5-small),
and repository tasks [0188](../../TASKS/0188-semantic-hardening-and-evaluation.md),
[0170](../../TASKS/0170-perceptual-image-duplicate-detection.md), and
[0220](../../TASKS/0220-exact-production-semantic-evaluation.md).
Google's ~14% code result compares **EmbeddingGemma 2 against EmbeddingGemma 1**,
not against E5. E5's published Mr. TyDi numbers and Gemma's multilingual
MTEB v2 numbers use different benchmarks and must not be compared head-to-head.
