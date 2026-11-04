# 0298 Corpus themes: folder scope selector and export (Markdown, Word, SKOS)

Status: open
Priority: medium
Subsystem: backend, frontend, semantic
Depends on: none

## Context
Semantic corpus themes (clusters) are computed over the whole library. With unrelated research
topics (e.g. crime and adolescence vs. TRIZ) the topics compete for the same centroids and the
clusters get muddy. Users also want to take the cluster results out of the app.

Only a single semantic library can be active, so the remembered folder choice is a **global**
semantic setting (not per workspace; workspaces are pane layouts and unrelated to the index).

## Acceptance Criteria
- Opening the themes view offers a multi-select of indexed (consented) folders, default all.
  Clusters are computed only over documents in the selected folders.
- The last selection is remembered as a global semantic setting; stale/removed folders drop out.
- Results are cached per folder-set so toggling back is instant; cache invalidates on index change.
- Export a single cluster or all clusters to Markdown, Word (.docx) and SKOS (Turtle default,
  optional RDF/XML and JSON-LD) via the backend; browser and Tauri hosts behave the same.
- Export content per cluster: label, keywords, description, document count, optional 3–5
  near-centroid passages (path + excerpt), optional member file list. Header has scope and date.
- Passages and file paths are excluded by default; an explicit "include passages" toggle enables them.
- SKOS: one ConceptScheme per export (scope + date, `skos:note` marking it auto-generated), one
  Concept per cluster with `inScheme`, `prefLabel` (label), `altLabel` (keywords), `definition`
  (description). `skos:related`/none for flat clusters; `broader`/`narrower` only if hierarchical.
  Stable `urn:uuid` concept URIs, optional user-supplied base URI. Passages/paths only via
  `skos:example`/`dct:source` behind the include-passages toggle.
- Tests: scoped clustering excludes other folders; per-format renderers (golden output); SKOS
  parses back; i18n in all 8 locales; `pnpm api:check` clean.

## Implementation Notes
- Existing code: `crates/fm-application/src/corpus_themes.rs`, `crates/fm-transport-dto/src/
  corpus_themes.rs`, worker side `crates/fm-semantic-worker/src/{kmeans,corpus_theme_ipc}.rs`.
  SKOS vocabulary handling exists in `semantic_vocabulary.rs` (see 0187/0242) — reuse its types
  and vocabulary where possible.
- Scope filter uses the app's verified stable document identities; the worker never crawls paths.
- One export model ("cluster document"); each format is a renderer, so a new format is one renderer.
- Word generation in the backend (Rust docx writer), not the frontend.
- Folder scope belongs in the semantic settings (migration machinery), see 0297.

## Agent Notes
- Created from a design discussion; code not yet inspected beyond locating the files above.
  Open check: confirm no multi-library support exists (user states only one library is active).
