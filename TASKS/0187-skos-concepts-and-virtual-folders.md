# 0187 SKOS vocabularies and concept virtual folders

Status: open
Priority: medium
Subsystem: backend, frontend, metadata
Depends on: 0162, 0182

## Context

SKOS vocabularies can label indexed documents with stable concepts and connect semantic content to
Procyon's virtual folders. A single global generated taxonomy would mix unrelated domains and make
machine suggestions look authoritative. Vocabularies should instead be named, attachable resources
whose imported/user-accepted concepts are authoritative.

Concept labelling is derived metadata. Changing a vocabulary must replace concept annotations
without changing document embeddings or reprocessing source files.

## Acceptance Criteria

- Named device-local vocabularies import/export a documented SKOS subset with stable concept URIs,
  preferred/alternate labels, definitions/scope notes, broader/narrower, and related links. Parsing
  rejects malformed cycles/identities with actionable diagnostics and preserves unknown safe fields
  according to a documented policy.
- Vocabularies attach independently to enrolled roots and workspaces; multiple vocabularies may
  apply. Virtual folders persist vocabulary ID + concept URI, never a copied mutable display label.
- Imported and explicitly accepted concepts are authoritative. Locally extracted candidate labels,
  synonyms, and relationships enter a review queue with supporting chunks, confidence, corpus
  frequency, and accept/edit/reject actions; they are never silently published.
- Accepted concept labels/definitions are embedded locally once using the active library model.
  Changed/new document chunks are matched in a separate resumable post-index phase with documented
  thresholds and bounded candidate counts.
- Chunk occurrence metadata stores replaceable concept IDs and evidence/confidence. Vocabulary
  edits or threshold changes relabel affected chunks without changing chunk vectors; publication is
  generation-based so readers never see a partially updated vocabulary.
- Low-confidence matches remain review suggestions rather than active labels. Users can inspect why
  a document was labelled and navigate to supporting chunks.
- Concept virtual folders use the existing structured saved-search/virtual-location model, support
  broader/narrower inclusion choices, paging/cancellation, stable file results, provider scope, and
  unavailable/stale coverage.
- Optional LLM-assisted hierarchy or synonym suggestions use an explicitly selected 0184 profile,
  remain review-only, carry provenance/model version, and do not block statistical/embedding-only
  vocabulary maintenance.
- Source changes mark affected labelling dirty and coalesce rebuilds. Exclusion deletes concept
  evidence for that scope; vocabulary deletion confirms affected virtual folders and annotations.
- Ordinary backup includes vocabulary sources, accepted edits, review decisions, and attachments;
  replaceable concept embeddings/annotations may be rebuilt.
- Tests cover SKOS round-trip, stable identities, invalid relationships, attachments, multilingual
  labels, deterministic candidate extraction, review lifecycle, vector-preserving relabelling,
  generation publication, source changes/deletion, virtual-folder navigation, hierarchy expansion,
  and tenant/access isolation.

## Implementation Notes

- Keep the authoritative vocabulary/evidence store outside Zvec; use Zvec for concept embeddings and
  filtered retrieval. Concept IDs in chunk records are derived annotations.
- Do not append accepted labels or definitions to document embedding text. Concepts affect filters,
  boosts in a future hybrid task, browsing, and explanation without invalidating base vectors.
- Review the proven generation/fingerprint ideas in `~/dev/scw/rag` tasks 0103-0105, but adapt them
  to Procyon's Rust, VFS, workspace, and virtual-folder boundaries rather than copying Python code.

## Agent Notes

- 2026-09-04: Split from 0176. Agreed policy is named attachable vocabularies, curated authority,
  reviewed machine suggestions, and automatic replaceable post-index labels.