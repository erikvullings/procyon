# 0284 Corpus themes and 2D embedding cluster map

Status: in_progress
Priority: medium
Subsystem: backend, frontend
Depends on: 0185, 0283

## Context
Chunk embeddings already exist in the semantic store (normalized, fixed dimension, per model
profile) and task 0185 runs deterministic per-document k-means to pick representative chunks
(`crates/fm-semantic-worker/src/representative_selection.rs`). We want, like Weaviate's centroid
and cluster views, to expose document-level centroids, discover themes/topics across a whole
corpus (a library, folder scope or JSONL feed), label them, and show a 2D cluster map that links
back to sources (file + chunk; line number for JSONL records, task 0283).

## Design decision: centroid vs chunk embeddings
Cluster **chunk embeddings directly** for theme discovery; also keep a **document centroid**
(mean of chunk vectors, re-normalized) for a different job. Reasons:
- Documents are multi-topic, and a single JSONL file holds thousands of unrelated messages, so a
  document centroid blurs distinct topics and is meaningless for feeds. Chunks are the unit that
  carries one topic.
- A document's themes are expressed as a **distribution over chunk clusters** (theme mixture),
  not one centroid assignment.
- The document centroid stays useful for: document-level 2D map, "similar documents", cheap
  incremental updates, and as the clustering input for very large corpora (cluster centroids or a
  sample first, then assign remaining chunks to the nearest cluster).

## Acceptance Criteria
- Worker computes and persists (derived, rebuildable, tagged with embedding model/profile +
  normalization + index revision) per-document centroids; invalidated incrementally when a
  document's chunks change; never mixes vectors of different model profiles.
- Corpus clustering in the worker over chunk embeddings (scope = library / folder scope,
  respecting consent, folder exclusions and revocation): density-based (HDBSCAN-style, with a
  "no theme" outlier bucket) or deterministic k-means with automatic k; deterministic given a
  seed; bounded memory/time with sampling + nearest-centroid assignment above a size threshold;
  cancellable; resumable/idempotent; typed outcome when the active profile is too weak
  (token-hashing compact profile) to give meaningful themes.
- Theme labels: top c-TF-IDF keywords per cluster, the nearest-to-centroid representative
  chunk(s) with provenance, optional SKOS vocabulary matches (0242); no LLM required.
- Document theme mixture: per-document distribution over themes.
- 2D projection (deterministic PCA baseline; UMAP/t-SNE optional, seeded) returned as points
  {x, y, theme, document, chunk ref, provenance} with a bounded point count (sampling/LoD).
- API/DTOs (OpenAPI regenerated via pnpm scripts, never hand-edited), mock/http/tauri parity;
  vectors never leave the worker/host boundary beyond projected 2D coordinates.
- Frontend: "Themes" view with theme list (label, size, keywords, sample), a virtualized/canvas 2D
  scatter map coloured by theme, hover/click opens the source at the chunk (line for JSONL),
  filter by theme feeds search; stale-index indicator; i18n in all 8 locales; accessible list
  alternative to the map.
- Tests: deterministic clustering on synthetic embeddings with known groups (including
  interleaved/unsorted input and multi-topic documents), centroid invalidation, profile-mismatch
  refusal, exclusion/revocation, projection bounds, UI model tests.

## Implementation Notes
- Reuse k-means in `representative_selection.rs`; storage in `semantic_storage.rs`.
- Slices: (1) centroids + corpus clustering + labels (worker + API), (2) 2D projection + Themes
  view, (3) theme-filtered search and incremental recluster.
- Quality depends on a real embedding model (EmbeddingGemma), not the compact hashing profile.

## Agent Notes
- 2026-10-25 claude: task created from user question; not started. Decision recorded: cluster
  chunks directly, centroids per document for map/similarity/large-corpus bootstrap.
- 2026-10-25 claude: slice 1 (centroids + corpus clustering + labels, worker + API) built on branch
  `feat/0284-corpus-themes`. Worker: `corpus_themes` (deterministic k-means with automatic k by
  silhouette over a bounded sample, stride-sampled fit above `max_fit_chunks` then nearest-centroid
  assignment of every chunk, per-cluster radius -> "no theme" bucket and dissolved tiny clusters,
  c-TF-IDF keywords, nearest-to-centroid passages with provenance, SKOS concept counts from
  `records.concept_id`, per-document mixtures, typed `ProfileTooWeak`/`ProfileMismatch`/`EmptyScope`/
  `InsufficientContent`, cancellation, idempotent persisted run keyed by a fingerprint of model +
  config + index revision), `corpus_theme_storage` (scope-bound chunk streaming over complete
  generations of available occurrences only, `document_centroids` + `corpus_theme_runs` tables,
  centroid tagged with model/normalization/algorithm/generation/content hash, discarded on
  `delete_occurrence`/reclaim) and a shared `kmeans` module that `representative_selection` now
  reuses (its tests unchanged). Application: `CorpusThemeCapability` seam (inert by default, like the
  summary/RAG seams: hosts do not inject a worker yet, so HTTP/Tauri answer 503 until the worker
  activation wiring exists), scope re-authorized through `resolve_knowledge_scope`, empty authorized
  set never widens. API: `POST /api/v1/semantic/themes/{status,analyze}`, Tauri commands, mock/http/
  tauri client parity, OpenAPI regenerated. Vectors never leave the worker; DTOs carry keywords,
  passages, mixtures only. Verified: 13 worker tests (`cargo nextest run -p fm-semantic-worker
  corpus_`), 2 application integration tests (`--test semantic_indexing corpus_themes`), 3 mock
  client tests; workspace clippy -D warnings, biome, tsc clean. Known limits: host entire-library
  scope covers the requesting workspace tenant only (like knowledge search); projection, Themes view,
  theme-filtered search and incremental recluster are slices 2-3 (not started).
