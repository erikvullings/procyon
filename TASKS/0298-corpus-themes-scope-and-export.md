# 0298 Corpus themes: folder scope selector and export (Markdown, Word, SKOS)

Status: done
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
- 2026-11-04 claude: Implemented.
  - Scope: the backend already accepted `RagScopeDto` (`enrolledRoots` + `enrolledRootIds`), so no new
    clustering path was needed. The worker already caches one run per sorted source set
    (`scope_key`) and the fingerprint includes the index revision, which satisfies "cached per
    folder-set, invalidated on index change". Added the global setting `semantic_theme_root_ids`
    (`fm-settings`, `SettingsDto`, `settings_mapping.rs`, additive via `serde(default)`, no schema
    bump). `themes-view.ts` loads the roots, validates the remembered ids (`validRootSelection`),
    shows a checkbox panel with an explicit Apply (recomputing on every toggle is expensive), and
    stores `[]` when every root is chosen so newly added folders are included by default.
  - Export: `crates/fm-application/src/corpus_theme_export.rs` (pure renderers over one export
    model: Markdown, Word via ferrodoc HTML to docx, SKOS Turtle / RDF/XML / JSON-LD, plus
    `write_export` through the provider `WRITE` path). `FileManagerService::export_corpus_themes`
    re-authorizes the scope and re-runs the analysis on the backend; nothing in the file comes from
    the caller except the labels the user saw. Route `POST /api/v1/semantic/themes/export`, Tauri
    command `export_corpus_themes`, mock + HTTP + Tauri clients, OpenAPI/Orval regenerated.
    The file is saved in the folder the Themes tab was opened from, never replacing a file.
  - Verified: 12 renderer unit tests (`cargo test -p fm-application --lib corpus_theme_export`),
    1 integration test (`--test semantic_indexing corpus_theme_export_is_scoped...`: only the
    chosen root's sources reach the worker, passages opt-in, no overwrite, unknown theme and empty
    scope rejected without writing), 8 new view tests + 4 model tests (`themes-view.test.ts`,
    `themes-model.test.ts`). Full `cargo nextest run -p fm-application -p fm-settings
    -p fm-transport-dto -p fm-server` 1632 passed; full frontend vitest 3372 passed; `tsc --noEmit`,
    `cargo clippy --workspace --all-targets -D warnings`, biome (14 pre-existing warnings) clean.
  - Known gaps / deviations (not silent):
    - File **paths** are not exported: the analysis only carries opaque source ids and titles, so
      "path" is the document title (+ section path). SKOS therefore uses only `skos:example` for
      passages, not `dct:source`.
    - The description is generated from counts and keywords (no LLM), in English; the file notices
      are English too. Keywords are filtered with a minimal rule (>= 3 chars, not numeric) while the
      on-screen label drops stop words, so the label is sent from the UI (`labels`) to match it.
    - The Turtle output is asserted structurally, not parsed by an RDF parser (none in the
      workspace); RDF/XML is parsed as XML and JSON-LD as JSON. Word output is checked by reading
      `word/document.xml` text, not opened in Word.
    - Not run in a real desktop/browser session: the new panels' appearance and the Materialize
      checkbox/select overrides in `theme.css` are untested visually.
    - Hierarchy: clusters are flat, so no `broader`/`narrower` are written.
