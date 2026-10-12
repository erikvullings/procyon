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

- 2026-10-12 Copilot: The opt-in worker now routes a completed, digest-verified
  >64-MiB anonymous PDF spool through the bounded text child and passes the
  reconstructed report into the normal ingestion coordinator without a
  second host-side source copy. Feature-gated authenticated IPC tests cover
  spool admission, missing backend, consented-image refusal and page-cited
  backend input; the pipeline's atomic-generation test preserves a prior
  complete generation when a replacement report is partial. An ignored
  fixture test streamed the 188,870,838-byte synthetic PDF through worker
  IPC and confirmed 354/354 page evidence at the backend; a second ignored
  fixture test embedded the child output through the real generation
  pipeline and confirmed 354 visible page records with first/last page
  citations. **Only** the development
  `experimental-large-pdf` feature enables this path, and its sampled
  Mac memory watchdog and text-only behavior are not release-qualified.
  Gemma libraries with image consent still reject a large PDF rather than
  publish text while omitting its images. The production worker recipe,
  native installed app, scans, durable UI coverage and four-target evidence
  remain no-go; no release/tag/candidate was dispatched.
- 2026-10-12 Copilot: Behind the off-by-default
  `fm-semantic-worker/experimental-large-pdf` feature, the real worker
  executable now has a path-free PDF text child and a supervisor that
  streams an anonymous spool, checks its SHA-256 on both sides, samples
  child RSS, kills/reaps on cancellation/deadline or excessive RSS, and
  validates a capped typed text report. The 188,870,838-byte synthetic
  fixture passed through that executable, producing text with citations
  for all 354 pages; `/usr/bin/time -l` reported 1.76 s and 402,096,128
  bytes maximum RSS for the test process group on this Apple Silicon host.
  The provisional 512-MiB RSS threshold is **sampled**, not OS-enforced
  and not four-target qualified. The production IPC completion still
  rejects >64 MiB; ingestion, atomic publication, image windows and
  release packaging do not enable this experimental child. No source
  PDFs are retained after the synthetic test.
- 2026-10-12 Copilot: `merge_pdf_text_windows` now accepts only adjacent,
  ordered PDF text windows covering every declared page, with the exact
  expected per-window `ItemsDropped` omission and no other truncation or
  visual omissions. It rebases unit order while retaining original page
  citations and enforces combined document character/unit/warning budgets.
  Missing, overlapping, budget-exceeding, or visually incomplete windows
  cannot be promoted to a complete document. This is a conversion-stage
  primitive; the 188,870,838-byte synthetic text fixture also passed
  twelve 32-page-or-smaller windows inside the killable probe child,
  producing all 354 parent-validated page citations in 1,756 ms parent
  wall time with 397,115,392 bytes sampled child peak RSS. Low-RSS
  and cancellation probes still killed/reaped the windowed child.
  The worker still needs a bounded child-result transport,
  cancellation-aware atomic staging, and spool admission before large PDF
  text is searchable.
- 2026-10-12 Copilot: A path-free `PdfTextReport` now serializes only a
  complete baseline PDF's ordered text units, page/block citations and
  conversion warnings. The receiving side validates page/character/unit
  budgets and reconstructs the same versioned `ConvertedDocument` for the
  normal chunker; malformed citations, kinds, order or oversized text fail
  closed. In the synthetic child probe, all 354 pages round-tripped through
  a 50,339-byte report with 397,131,776 bytes sampled child peak RSS.
  Authentication/source digest, actual worker child-result transport and
  atomic publication remain separate unimplemented release prerequisites.
- 2026-10-12 Copilot: A synthetic-only conversion probe
  (`cargo run -p fm-semantic-conversion --example measure_large_pdf_text -- <fixture>`)
  checks that the generated 354-page, 188,870,838-byte text fixture
  (SHA-256 `b1e36f4b3e4664d00e09475372f9e90858aaef0adfab3762aaa0ea98cc4d2285`)
  yields text on every page with no truncation. On this macOS arm64
  development host a debug-build direct process converted it in 234 ms,
  reaching 389,808,128 bytes maximum resident set size (`/usr/bin/time -l`).
  A follow-up synthetic-only probe now streams the source to a kill-on-drop
  child without copying it into its parent, checks the digest on both
  sides, polls child RSS every 10 ms and enforces a 120-second deadline.
  The full fixture completed in 919 ms parent wall time, with 354/354 text
  pages and 395,034,624 bytes sampled child peak RSS; `/usr/bin/time -l`
  reported a 2,949,552-byte peak memory footprint for the parent command.
  `--verify-kill` confirmed a deliberately low 32-MiB sampled threshold
  kills and reaps the child; `--verify-cancel` aborts and reaps an
  in-progress synthetic conversion. The child also returns a 25,627-byte capped
  page-text report over stdout, validated in the parent for ordered page
  citations 1-354 and each page's synthetic text; sampled peak child RSS
  was 395,083,776 bytes in that run. Parent-side output reads stop at 8 MiB.
  The example is **not the production worker path** and a sampled
  watchdog is not an instantaneous hard OS memory cap. It excludes IPC,
  embeddings, image extraction, atomic staging, worker IPC cancellation and
  four-target resource qualification; it does not permit >64 MiB
  production admission. Generated fixtures were removed after the runs.
- 2026-10-12 Copilot: Directly drawn 8-bit DeviceRGB/DeviceGray Flate
  images now accept validated PNG predictor parameters (10-15), with
  compressed-row overhead included in the decompression bound. A synthetic
  predictor-12 image round-trips its exact pixels and page citation; mismatched
  columns fail as a reported omission. This improves consented embedded-image
  coverage but does not render pages, lift the 32-image or 64-MiB source
  limits, or qualify a large scan.
- 2026-10-12 Copilot: A prototype child consuming the anonymous PDF spool
  was removed after finite `RLIMIT_AS` and `RLIMIT_DATA` both returned
  `EINVAL` on the required macOS arm64 host; shell `ulimit -v/-d/-m`
  independently rejected those limits. An alternative qualified process
  memory watchdog/boundary remains required before >64 MiB admission.
  Do not silently substitute an unbounded Mac process or treat sampled
  RSS as an instantaneous OS-enforced cap. Release remains no-go.
- 2026-10-12 Copilot: The preferred/advanced converter's bounded-byte
  preflight now borrows an already-materialized source instead of cloning
  the whole PDF before parsing. Reader inputs still materialize only under
  the existing source-byte limit. An ownership regression checks both
  paths. This removes one full input-sized allocation from the current
  <=64 MiB path and reduces a future child process's memory pressure, but
  does **not** make 189 MB parsing safe in the parent or establish a
  four-target memory ceiling.
- 2026-10-12 Copilot: Added an explicit one-based, validated PDF page window
  to the conversion context. The baseline converter validates the complete
  page tree before extracting a subset, retains original page citations,
  and marks every subset `Partial` with dropped-page accounting; the
  preferred converter delegates window requests to baseline rather than
  accidentally extracting the entire PDF. A 33-image synthetic scan
  fails whole-document conversion but can be extracted as 32+1
  bounded *partial* windows; ingestion still rejects publishing either
  window alone. This is a staging primitive, not a streaming parser:
  each invocation still loads the entire source into memory, the 32-MiB
  visual budget is per window, and no atomic window aggregation, killable
  child, >64 MiB admission or scan renderer exists yet.
- 2026-10-12 Copilot: The worker now transports a completed PDF's retained
  image/page omission summary as a separate optional `coverage_detail`
  field, never as a protocol error. Scoped catalog jobs and IPC regressions
  distinguish completed-with-omissions from failed/skipped; application
  reconciliation exposes bounded, sanitized `coverage_details` separately
  from `failure_details` on both desktop and browser-server hosts. This
  preserves truthful job-level disclosure, but does not yet persist a
  structured per-page coverage map, display an accessible frontend
  indicator, remove the 32-image cap or admit >64 MiB source PDFs.
- 2026-10-12 Copilot: Host reconciliation now preserves distinct worker
  failure reasons in `SemanticIndexingReport.failure_details` instead of
  reporting only a failed-occurrence count. A synthetic page-33 image-limit
  diagnostic survives the Tauri application feed while other files continue
  indexing; OCR remediation also retains worker failure detail rather than
  replacing it with a generic error. The one-shot report is not yet a
  durable per-page coverage model or an HTTP/Tauri/mock frontend indicator.
- 2026-10-12 Copilot: Converting a consented PDF with a 33rd supported drawn
  image now fails with its page/image location instead of publishing a
  deceptively complete first-32-image generation. The preferred Docling
  converter propagates the baseline visual-enrichment failure rather than
  discarding it after successful text extraction; a worker edit regression
  checks that the prior complete generation remains visible. Unsupported
  individual image encodings still carry explicit omissions. This closes
  one partial-publication path, not the 32-image whole-document limit: a
  354-page scan still needs bounded page-windowed conversion, coverage
  accounting, and >64 MiB process isolation before release.
- 2026-10-12 Copilot: Added an opt-in, pure-Rust path for simple directly drawn
  8-bit Flate RGB/gray PDF XObjects: decode at most 4 MiB of exact-size pixels,
  encode a bounded PNG, and retain `PdfImage` page provenance for Gemma's
  existing consent-gated embedding path. Unsupported predictors, color
  spaces, masks, encodings and raster sizes remain explicit omissions. Tests
  cover image-off, image-only and mixed-page conversion, pixel fidelity,
  malformed lengths and the worker's consented PNG record. This reconstructs
  a supported embedded raster; it does not render a PDF page, run OCR, support
  forms, or qualify the native PDFium supply chain. The existing 32-attachment
  and 32-MiB/document retention caps still prevent full 354-page scan
  coverage; >64 MiB ingestion and release remain no-go.
- 2026-10-12 Copilot: Baseline and Docling's initial PDF parse now cap eager
  decompression of each structural object/xref stream at 16 MiB. A synthetic
  sub-MiB PDF with an over-16-MiB compressed page object must fail as malformed,
  not become an OCR suggestion or publish partial text. Both converters now
  verify the declared page-tree count against the loaded pages; a split
  object-stream fixture that loses one of two pages cannot publish the
  surviving page as a complete document. This is a parser
  safety prerequisite, not a total memory bound: `lopdf` still materializes the
  source and the full document, and Docling may parse again. Production still
  rejects the >64 MiB spool until a measured, killable, resource-bounded child
  can consume it.
- 2026-10-12 Copilot: Corrected PDF image evidence so a JPEG is eligible only
  when the page content actually invokes its named XObject with `Do`;
  merely declaring an image in inherited `/Resources` no longer makes it a
  searchable page hit. A synthetic unused-resource regression failed before
  the fix, while drawn inherited JPEGs and consented worker image indexing
  still pass. Page content is inspected with the existing 16 MiB bounded
  decoder; JPEGs larger than the visual attachment limit are omitted before
  cloning their bytes. This does not render forms or scanned Flate pages,
  raise the 64 MiB production cap, or qualify the release.
- 2026-10-12 Copilot: PDF visual extraction now honors inherited
  `/Resources` and does not invent an image-inspection failure when a
  text-only page has no `/XObject`. The previous `lopdf::get_page_images`
  shortcut read only direct page resources, so it missed an inherited JPEG
  and classified a normal inherited font-only resource as an error.
  Image-only pages without a reusable image now carry explicit
  `PdfPage { page_number }` omission provenance; image/XObject failures retain
  distinct image provenance. Worker job detail separates page-coverage gaps
  from image omissions and keeps page reasons as examples. Synthetic converter
  and worker regressions pass. These counts are not yet projected into the
  host/frontend; page rendering and >64 MiB conversion remain disabled.
- 2026-10-12 Copilot: `node scripts/generate-semantic-pdf-fixtures.mjs
  <existing-temp-directory>` now generates deterministic, page-local 354-page
  image-only and text-layer PDFs on demand without checking large binaries
  into Git or overwriting existing files. The synthetic scans use Flate RGB
  XObjects, not JPEGs or a private document. A full run produced 173,969,316
  bytes (scan, SHA-256 `55731b028f572398ecf3063c24e5ade84e2116e8ac9e5a6818949f4d0f9eeb4a`)
  and 188,870,838 bytes (text, SHA-256
  `b1e36f4b3e4664d00e09475372f9e90858aaef0adfab3762aaa0ea98cc4d2285`).
  `pdfinfo` identified 354 pages in each, `pdftotext` found the final page's
  label in the text PDF and zero text in the scan; generated files were
  deleted after validation. This supplies reproducible target-sized inputs,
  **not** measured conversion memory or page-image coverage. JPEG, malformed,
  bomb, edit/deletion, resource and cancellation cases still need separate
  fixtures/qualification before raising the host limit.
- 2026-10-12 Copilot: PDF text conversion budget exhaustion now fails
  ingestion for image-off/E5 PDFs too, rather than publishing later pages as
  an apparently complete document. A two-page synthetic edit that exceeds
  the output budget leaves the previous complete generation visible and marks
  the new job failed. This closes a partial-publication path but does **not**
  raise the production 64 MiB limit: the worker still rejects a completed
  large-PDF spool because baseline conversion requires an in-memory byte
  slice, and no killable, memory-bounded converter child has been qualified.
- 2026-10-12 Copilot: Release packaging audit found an additional desktop no-go:
  `release-semantic-components.yml` signs workers for macOS arm64, Windows x64,
  Linux x64 and Linux arm64, whereas experimental `release-desktop.yml` builds a
  **universal** macOS application but fetches only the macOS arm64 worker/catalog.
  The pinned Zvec runtime inventory has no macOS x64 binary. The user chose an
  Apple Silicon-only experimental artifact, leaving the normal universal
  non-Gemma package and Intel updater intact. The release recipe now selects
  arm64-only Gemma bundles and an isolated experimental updater endpoint;
  signed/notarized installed behavior remains unverified. Do not infer Intel support
  or relax the exact-candidate gate. The pinned no-ML Docling converter has no
  page rasterizer; no native PDFium library, notices or renderer package is yet
  pinned across the signed worker targets, so scanned-page support is also no-go.
- 2026-10-12 Copilot: Fixed Knowledge's same-page PDF-image identity collision.
  Its previous de-duplication key used only document, record kind, generation and
  source position, so separate images on one PDF page collapsed into a single
  result. Exact image provenance now distinguishes images while identical
  image records across source occurrences still coalesce. A public retrieval
  regression with two interleaved image candidates and a duplicate occurrence
  failed before the change and passes after it, as does the worker library suite.
  The same regression exposed unrelated PDF text being added as adjacent context
  for an image only because image page numbers share the text chunk-position
  field. PDF image evidence now has no fabricated text adjacency, and text
  evidence cannot acquire a neighbouring image through that position collision;
  both directions pass through the public retrieval test and full worker suite.
  This does not change the production 64 MiB cap or the release no-go.
- 2026-10-12 Copilot: File-primary semantic results now distinguish an exact `pdf_image`
  match from source-level PDF text: the selected result shows a localized embedded-image
  page citation, does not fetch/display the whole PDF as an image, and opens its source
  viewer at the cited page rather than searching an English image excerpt as PDF text.
  The page is derived from validated exact provenance; malformed provenance cannot
  produce a fabricated page. Knowledge and file-primary evidence remain host-neutral.
  F3 on a selected, current PDF image result opens the opposite viewer on the same
  indexed page; ordinary file F3 continues unchanged, and stale/unavailable matches
  never supply an old page.
  The new file-primary regression passed; the complete 152-file frontend suite,
  frontend typecheck and workspace lint also passed.
  This UI change does not supply embedded-image preview bytes or cover scanned pages
  without an approved renderer. The rest of TASKS/0244 remains in progress.
- 2026-10-12 Copilot: Consented Gemma PDFs now stage bounded reusable JPEG XObject
  embeddings with the PDF text in one document generation. Exact one-based page/image
  provenance identifies `pdf_image` evidence; repeated JPEG bytes reuse a model- and
  preprocessing-bound vector. Tests cover image-off and E5 behavior, a mixed PDF,
  image-only JPEG pages, unsupported-page omissions and preservation of the previous
  generation on embed/publication failure. Knowledge results now label these records as
  embedded PDF images with localized page/image citations and open the source PDF on
  the cited page, without presenting a whole-PDF thumbnail as the indexed image.
  This does not enable page rendering, expose the stored embedded bytes for preview,
  or turn unsupported scanned pages into searchable results. Production remains at
  64 MiB and no release qualification is claimed.
- 2026-10-12 Copilot: The affected protocol, converter, worker and application Rust
  suites passed except an existing repository release-report assertion
  `repository_corpus_and_report_are_current` (`InvalidIdentity`: the checked-in E5
  report names converter `docling-pdf/1036000+baseline/2` and chunker `structural/3`,
  whereas the current production pipeline names `docling-pdf/1036001+baseline/3`
  and `structural/4`). All other affected Rust tests passed
  when that single assertion was skipped; 204 Knowledge/i18n frontend tests passed,
  including the new PDF-image evidence case. This report assertion must be reconciled
  before CI/release; skipping it is only diagnostic, not an approval.
- 2026-10-12 Copilot: Added a separate host PDF source-snapshot path selected only when an
  explicitly injected capability advertises more than 64 MiB. The enrolled provider stream is
  copied in 64 KiB windows to an anonymous tempfile while SHA-256 and exact length are measured;
  the one snapshot is rewound for each authorized workspace rather than cloning source bytes per
  workspace. The host's PDF reader capability carries the digest and immutable image consent into
  the worker client's bounded reader; short, oversized or modified streams fail before
  publication. A synthetic sparse 65 MiB local PDF test completes through the fake capability
  for two workspaces under both browser-server and Tauri service configurations; a 200 MiB+1
  source is skipped without reading it. Checksum mismatch retains old fake evidence, and a
  capability without the
  larger offer continues to classify both 65 MiB PDF and text as oversized. The installed
  production IPC capability still advertises only 64 MiB because worker conversion is not
  resource-isolated and presently rejects completed larger PDF spools. These tests establish the
  host/VFS/IPC *boundary*, not 174-189 MB real worker conversion or Gemma PDF image coverage.
- 2026-10-12 Copilot: Rechecked renderer supply chain without downloading or enabling native
  libraries. Pinned `docling-pdf` 1.36.0 uses native PDFium only behind its disabled `ml`
  feature; baseline does not render image-only pages, and a supposed pure-Rust renderer
  installer claim was not corroborated in the pinned source. The upstream `chromium/8086`
  `bblanchon/pdfium-binaries` release has one Sigstore/SLSA attestation listing per-asset
  SHA-256s and non-V8 macOS arm64/x64, Windows x64 and Linux x64 assets, but none is pinned,
  redistributed with its PDFium/third-party notices, signed into Procyon or measured.
  `docs/docling-pdf-evaluation.md` additionally lists Linux arm64 as a fifth *candidate*
  target; `release-desktop.yml` ships four triples, not Linux arm64. Availability is not
  qualification: leave scanned-page rendering and the release gate disabled.
- 2026-10-11 Copilot: Added a separately negotiated PDF-only IPC ceiling with old-worker
  negotiation defaulting to the unchanged 64 MiB stream limit; ordinary media, result streams,
  and 1 MiB frames remain bounded as before. An opt-in worker configuration spools an oversized
  PDF stream to an anonymous temporary file with a single active spool, cumulative byte budget,
  existing stream deadline and cancellation; completion currently returns an explicit
  `LimitExceeded` rather than materializing the file or reporting an accepted ingestion.
  Protocol boundary and worker IPC regressions cover the opt-in negotiation, non-PDF rejection,
  cancellation, and no-publication completion. The production worker advertises only the
  ordinary 64 MiB limit, and the host and converter remain capped at 64 MiB. This is a
  transport-safety milestone, **not** functional large-PDF ingestion: host VFS streaming,
  content-integrity checksum, disk-pressure limits, killable conversion/page isolation,
  multi-workspace publication and four-target resource measurements remain prerequisites
  before opting in or claiming coverage. No live user PDF or review profile was accessed.
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
- 2026-10-11 Copilot: The mixed synthetic PDF's original JPEG fixture failed visual header
  validation despite containing JPEG signatures. Replaced it with an original, valid 8x8 JPEG
  and added a start-of-frame regression. An evaluation-only converter probe now verifies two
  retained JPEG images with page provenance and explicitly reports the third page's non-JPEG
  omission; one text-bearing page is extracted. This verifies bounded XObject reuse, not PDF
  image embeddings or scanned-page rendering.
- 2026-10-11 Copilot: The host now snapshots immutable Gemma image consent under the same
  authorized worker-feed decision, restricts it to PDF entries, and carries the typed flag across
  authenticated IPC into the ingestion input. E5 and image-off Gemma stay false; non-PDF requests
  with a true flag are rejected. Deterministic library, host reconciliation, protocol and IPC
  regressions cover the boundary. The worker still does not enable visual conversion or publish
  PDF image vectors: consent propagation alone does not qualify this release prerequisite.
- 2026-10-11 Copilot: Read-only renderer research confirmed the normal build excludes
  `pdfium-render` (`docling-pdf`'s `ml` feature is off) and that the wrapper does not supply
  native PDFium. The upstream `bblanchon/pdfium-binaries` `chromium/8086` release provides
  all four required non-V8 targets, a Sigstore/SLSA attestation with per-asset SHA-256 digests,
  and bundled PDFium/third-party notices. PDFium and its Rust wrapper use permissive
  redistribution licenses; Procyon has not pinned or verified those assets, carried their
  notices, packaged/signed the library, or measured renderer isolation and memory. The pinned
  Docling 1.36.0 source calls PDFium for rendering under `ml`, but a current upstream installer
  comment claims pure-Rust page rendering; resolve that discrepancy against exact upstream
  source/release before choosing an integration path. Availability and licensing alone do not
  qualify a renderer or the four-target release.
- 2026-10-11 Copilot: Added a separate three-page synthetic `mixed` fixture: one full-page
  reusable JPEG, a text-bearing page with the same embedded JPEG as a figure, and a scanned
  page with a non-reusable Flate XObject. The repeatable fixture and its six generator tests
  establish inputs for future provenance, opt-in, deduplication, and renderer checks; they do
  not establish that any of those three image pages is embedded or searchable yet.
