# 0244 Bounded large-PDF text and Gemma page vision

Status: in_progress
Priority: high
Subsystem: backend, semantic, frontend, release
Depends on: 0177, 0179, 0180, 0182, 0183, 0192, 0194, 0195, 0227

## Context

Experimental Gemma release now requires searchable text from consented PDFs around 174-189 MB
(354 pages), plus image embeddings for both extractable embedded images and scanned/image-only
pages when useful text is absent. This is a prerequisite of 0239's release decision, not permission
to publish the existing signed candidate. E5 remains the default text model.

Today the host reads at most 64 MiB; IPC caps a complete stream at 64 MiB; worker conversion also
caps source input at 64 MiB. Raising just the host constant cannot ingest these files. PDF
conversion currently emits text chunks, while PDF visual evidence is opt-in for summaries and is
not passed to the Gemma media embedder. The existing visual extractor retains only reusable JPEG
XObjects, at most 32 attachments of 4 MiB each and 32 MiB total; it does not render pages. A
standalone page PNG may therefore be indexed while its source PDF is absent from search.

## Acceptance Criteria

- Deterministic, redistributable synthetic fixtures exercise at least a 354-page, approximately
  174 MB scanned PDF and an approximately 189 MB text-bearing/OCRed PDF; they include page-local
  text, reusable and non-reusable embedded images, image-only pages, malformed/compression-bomb
  negatives, cancellation, content edits, and deletion. Never process or copy private user PDFs
  without a separate explicit consent decision. Measure wall time, peak host and worker RSS,
  temporary disk, indexed page/image coverage, and query/citation behavior on all four supported
  targets; set and enforce a release memory/time ceiling from that evidence, not an invented one.
- Provider-neutral host enumeration alone selects files under enrolled roots, checks eligibility,
  retains exact workspace/tenant consent, and streams bytes through a bounded authenticated IPC
  capability. A worker-owned, owner-only temporary spool can hold a PDF but never opens the
  original path, crawls a provider, accepts an external path, or retains data after revocation.
  Protocol negotiation restricts larger payloads to PDF ingestion without relaxing ordinary
  media or message bounds. Enforce a reviewed per-PDF ceiling covering the target fixtures (200
  MiB is a candidate, not an approved one), 1 MiB-or-smaller frames, checksums, cumulative
  limits, cancellation, and crash-safe cleanup.
- PDF conversion runs with a hard watchdog/resource boundary, bounded pages, decompressed bytes,
  pixels, extracted characters and per-page duration; progress and checkpoints permit resuming
  unfinished work without publishing a partial document generation. A failure names the page and
  reason, preserves the previous complete generation, and never reports an unembedded page as
  searchable. Keep baseline text-layer extraction for E5 and Gemma; OCR remains an explicitly
  installed/consented option and must never run silently.
- A Gemma library with its existing immutable **images** choice off indexes no PDF image vectors.
  With images on, embed eligible PDF XObjects with page/image provenance and render scanned or
  image-only pages lacking useful extracted text when an approved renderer is installed. Bound
  each rendered image by pixel/encoded-byte limits and use one-page windows; never silently
  truncate the 354-page fixture. Unsupported image encodings/pages have explicit omissions and
  coverage counts. De-duplicate a full-page embedded image and its rendered fallback; cache
  fingerprints exclude paths but bind bytes, converter/render version, model and preprocessing.
  Use the existing Gemma image encoder with verified Metal automatic selection and CPU fallback;
  do not change E5's vector space or pretend text chunks are image embeddings.
- Publish PDF text and image records together under one authorized document generation with
  exact page-aware citations, source navigation, root/workspace filters, current-content checks,
  cancellation and deletion/revocation cleanup. Mixed-media Knowledge/file-primary results must
  identify PDF page image evidence honestly; HTTP, Tauri and mock share the same DTO semantics
  and accessible EN/NL coverage/failure indicators. Generate API artifacts from the backend.
- Do not enable release until four-target signed candidate, installed consented host/VFS/IPC
  fixture, fallback, upgrade/rollback, cross-host result navigation, memory/deadline/omission
  tests and normal latest-head CI pass. Bind the candidate to the final reviewed source SHA.
  Existing noGo evidence and old signed assets remain unchanged; no tag, publish or merge.

## Implementation Plan

1. **Prove budgets and transport.** Use synthetic fixtures to measure page extraction and peak
   memory before setting any >64 MiB cap. Introduce a PDF-only negotiated stream/spool boundary
   with per-stream checks, deadline, owner-only cleanup, and a deterministic failure fixture.
   Do not materialize and clone an entire 200 MiB VFS file once per workspace. Keep non-PDF limits
   unchanged. Add protocol and application regressions for 64/200 MiB boundaries and cancellation.
2. **Incremental PDF text.** Reuse pure-Rust Docling text ordering and `lopdf` structural/page
   parsing while isolating large-PDF parsing in a killable, memory-bounded process. Checkpoint
   completed page outputs in worker-owned staging, then atomically publish only when all required
   pages complete or explicitly report bounded omissions. Handle changes and multi-workspace
   occurrences without orphaning cached vectors or claiming partial search success.
3. **Gemma PDF vision.** Reuse the existing `VisualProvenance::PdfImage`, JPEG XObject extraction,
   visual size/pixel checks and Gemma `embed_media` path. Add a vetted, separately signed page
   renderer only if necessary for full image-only/non-reusable image coverage on every supported
   target; render one page at a time with explicit time/memory bounds. Define PDF-page raster
   provenance, deduplication and one-generation text/image publication. Test an embedded figure,
   a full-page scan, a mixed text/image page, unsupported encoding, opt-out, stale content and
   deletion. Do not conflate summary visual attachments with searchable image records.
4. **Host/UI and qualification.** Project per-page progress, durable failure/omission counts and
   correctly authorized PDF evidence in all adapters. Verify installed CPU/Metal fallback, exact
   host-path search/citation/deletion and four-target packaging, then build a *new* read-only signed
   candidate after source stabilizes and latest CI passes. Manual release GO remains separate.

## Dependencies and licensing

Existing `lopdf` 0.44.0 is MIT licensed; the current Docling PDF crates 1.36.0 are MIT licensed.
No dependency is added by this task plan. If page rendering requires `pdfium-render` 0.8.37, its
published crate license is `MIT OR Apache-2.0`; its presence in Cargo.lock does **not** mean the
normal build currently includes it. PDFium's upstream LICENSE permits redistribution with notice
conditions and includes third-party terms. Audit the exact native library and full third-party
notices, source/version/hash, four target binaries, signed catalog/installer packaging, and sandbox
behavior before adding or enabling either wrapper or native renderer. Do not assume an existing
optional Docling ML/PDFium pack is qualified on those targets.

## Agent Notes

- 2026-10-11: User explicitly made large-PDF text and PDF image embeddings release prerequisites.
  Read-only live diagnosis: 354-page `cropped/input.pdf` is 173,653,067 bytes and skipped at 64
  MiB; a separate text-bearing OCRed PDF is 188,589,866 bytes but outside the enrolled scope.
  Only standalone PNG page images are searchable today. The private files were not copied or
  processed for this task; all new performance fixtures must be synthetic until separate consent.
  Preserve the isolated development app and its profile. Begin with the budget/renderer
  qualification before raising limits; report any resource or cross-target supply-chain blocker
  rather than weakening noGo.
- 2026-10-11: Added a first, non-production fixture slice in
  `scripts/generate-large-pdf-fixture.mjs` with tests. `--variant scanned|text --output <new-path>`
  defaults to 354 independently generated synthetic pages with embedded compressed image
  XObjects; `text` also adds a synthetic text layer. The generator refuses to overwrite files or
  exceed a 200 MiB *fixture-only* bound; this does not change any ingestion limits. Local `pdfinfo`
  verified 354 pages in both variants (174,224,803 and 189,239,712 bytes respectively), while
  `pdftotext` page 5 returned no text for `scanned` and the synthetic marker for `text`. Both
  generated files were removed after checking. Remaining: mixed/unsupported image, malformed
  and compression-bomb negatives, cancellation/edit/delete fixtures; resource measurements,
  transport, text/vision ingestion, host parity, renderer supply-chain and signed qualification.
  Neither fixture demonstrates the actual private PDF's extraction or searchability.
- 2026-10-11 Copilot: Extended the synthetic generator with a page-local text edit, malformed
  cross-reference and 17 MiB decompressed-page negative; five fixture tests pass. An
  evaluation-only `fm-semantic-docling` example accepts just the known generated fixture names
  under `target/` and raises its **local probe context only** to 200 MiB; production host, IPC
  and converter limits remain 64 MiB. On macOS arm64 the 189,239,712-byte text fixture converted
  354/354 text pages with no omissions in 0.25 s and a 974,028,800-byte maximum resident set; the
  174,224,803-byte image-only fixture returned `NoTextLayer` in 0.13 s with 898,596,864-byte
  maximum resident set. `pdfinfo` and one-page `pdftotext` used approximately 10-11 MiB RSS, but
  those external tools do not represent Procyon's converter, VFS, IPC or Gemma processing costs.
  Generation took 2.61/2.78 s and 129/132 MiB peak RSS respectively; the two PDFs occupy
  363,464,515 bytes on disk before cleanup. Fixed a measured negative: a compressed-page bomb
  previously returned generic OCR guidance; both baseline and preferred converters now report the
  unreadable page and decompression limit without suggesting OCR. Verified five fixture tests,
  one evaluation-example guard test, the complete 130-test conversion and 14-test Docling library
  suites, 132 script tests, affected crate typechecks and repository lint; these are suite totals,
  not counts of tests newly added here. These figures cover only deterministic synthetic bytes on
  one machine, not worst-case
  decompression, four-target RSS, embedded-image coverage, worker memory, or a production ceiling.
  No private PDF was opened; no production limit, package, candidate or release gate was changed.
  Remaining release blockers include an isolated killable converter, negotiated PDF-only spool,
  bounded renderer with audited four-target packaging, atomic text+vision staging, host/UI
  coverage and exact signed installed qualification.
- 2026-10-11 Copilot: A synthetic two-page PDF regression exposed partial success when the first
  page exceeded its decompression limit and the second remained readable. Baseline extraction now
  fails the whole replacement with a page-specific error; optional ML Docling likewise stops on
  a page-conversion error instead of accumulating readable pages and publishing omissions. A
  worker ingestion regression confirms a failed PDF replacement retains the previous complete
  searchable generation and records a failed job. These checks do not establish full coverage for
  pages with empty or unextractable text, nor remove the production 64 MiB source cap.
