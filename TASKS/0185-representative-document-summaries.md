# 0185 Representative document summaries

Status: open
Priority: medium
Subsystem: backend, frontend, rag
Depends on: 0182, 0184

## Context

Users should be able to create and discover a document summary without sending an entire large
document to an LLM. Existing content-chunk embeddings can select representative evidence locally:

```text
content chunks -> embeddings -> k-means clusters -> medoid per cluster -> LLM summary
```

The LLM receives real selected chunks, not synthetic centroids. Summary output is derived evidence:
it must retain links to supporting source chunks, must not recursively summarize itself, and must
not replace the original chunks as the basis for grounded claims.

## Acceptance Criteria

- A Summarize action is available for supported indexed documents from file actions, preview/lister
  surfaces, and semantic results. It requires an active LLM profile and shows scope, profile,
  local/cloud status, estimated representative input, and cloud disclosure before generation.
- Short documents whose complete content fits the configured summary input budget skip clustering
  and use all content chunks. Large documents derive a bounded cluster count from that token budget
  with documented minimum/maximum limits.
- K-means is deterministic for the same document generation, embeddings, algorithm version, and
  budget: fixed seed/initialization, bounded iterations, stable tie-breaking, and explicit handling
  of empty clusters and duplicate vectors.
- Each cluster selects the actual chunk nearest its centroid (the medoid candidate), never centroid
  text. Selection records cluster population/weight and distance. Representatives are deduplicated
  and ordered by original source position before prompt assembly.
- Selection preserves identified title/introduction and conclusion coverage where available and
  otherwise uses only clustering. Cluster weights keep a small outlier cluster from appearing as
  important as a dominant theme. No selected chunk is character-clipped after structural chunking;
  the packer reduces count within the token budget.
- The prompt identifies opaque source labels, section paths, provenance, and cluster weight; treats
  excerpts as untrusted evidence; requests a concise overview and a fuller structured summary; and
  forbids unsupported detail. Cloud requests use the minimized metadata policy from 0184.
- Summary records store document generation/content hash, profile/model identity, algorithm/prompt
  version, selected supporting chunk IDs and weights, creation time, and both brief/full text.
- A generated `document_summary` chunk is embedded with the active local embedding model and stored
  in Zvec with its source occurrence and provenance links. Summary chunks are excluded from
  clustering, concept-candidate extraction unless explicitly designed later, and recursive summary
  input.
- Semantic search can retrieve a summary, labels it as generated, groups it under the source file,
  and offers Open summary and Open source evidence. Preview surfaces expose the current summary
  without disguising it as source text.
- Grounded RAG may use summary text for document selection/context compression, but citations resolve
  to supporting source chunks. Generated summary prose is never the sole primary citation for a
  substantive claim.
- A source-generation change marks the old summary stale and leaves it visible until a replacement
  publishes atomically. Regenerate reuses medoid selection when its complete fingerprint matches.
  Enrolment exclusion deletes summaries and pinned derived evidence.
- Without an LLM profile, the UI may expose the deterministic representatives as Key passages but
  must not call them an AI summary. Generation failure leaves the base semantic index usable.
- Tests cover short/all-chunk input, deterministic clustering, empty/duplicate clusters, structural
  coverage, weighting, token packing, source order, prompt-injection-shaped evidence, summary
  metadata/provenance, Zvec discovery, RAG citation resolution, stale regeneration, cancellation,
  cloud minimization, and deletion.

## Implementation Notes

- Reuse already persisted chunk embeddings; do not create a second embedding pass merely for
  clustering. Run CPU-bound clustering in the worker with cancellation and resource bounds.
- K-means followed by nearest-to-centroid representative selection is sometimes called
  k-medoids informally, but this task does not require the iterative PAM algorithm. Name types and
  diagnostics precisely to avoid implying a different optimization.
- Keep summary generation in Procyon, which owns the LLM profile and credential. The worker can
  return representative chunks and later accept the generated summary for local embedding/storage.

## Agent Notes

- 2026-09-04: Added from the proposed representative-summary pipeline. The central safeguards are
  deterministic selection, structural coverage, cluster weighting, source-order prompts, summary
  provenance, and citations that resolve back to original chunks.
