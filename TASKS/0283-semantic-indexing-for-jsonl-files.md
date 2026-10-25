# 0283 Semantic indexing for large JSONL files

Status: done
Priority: medium
Subsystem: backend
Depends on: none

## Context
`.jsonl` (one JSON object per line) files, e.g. exported channel/article feeds with `title`, `text`,
`summary`, publication date, channel name and base64 image data, are not semantically indexable
today (`.json` maps to `application/json` in `fm-application/src/semantic_indexing.rs` ~L996; the
converter in `fm-semantic-conversion` is bytes-in with `max_source_bytes` 64 MiB). Files can be
arbitrarily large (GBs), so a whole-file in-memory conversion is not acceptable: **no file-size
limit**, bounded memory.

## Acceptance Criteria
- `.jsonl` (and `.ndjson`, `application/x-ndjson`) is recognised during semantic eligibility
  (extension + sniff) and routed to a dedicated `jsonl` converter in `fm-semantic-conversion`
  (`formats/jsonl.rs`, new `FormatKind`), not the plain-text/`json` path.
- One JSON object per line becomes one (or more, if long) searchable unit(s). Selected fields are
  indexed: `title`, `text`, `summary`, publication date (`date`/`published`/`pubDate`/`published_at`
  and similar), channel name (`channel`/`channel_name`/`source`), plus other short string fields
  when no known fields exist. Field mapping is data-driven with sensible defaults.
- Provenance preserves the 1-based **line number** (and byte offset) of the source record so results
  can point back to the record; chunk fingerprints still exclude file names/paths.
- Base64/data-URI blobs and other large opaque values (e.g. image fields, strings that are long
  base64) are omitted; repeated metadata is not duplicated per chunk (e.g. emit channel/date as unit
  metadata/heading once per record rather than repeating verbatim in every chunk; do not re-emit
  identical per-record constants).
- Malformed lines (invalid JSON, non-object, invalid UTF-8, over-long line) are skipped, counted and
  reported as typed warnings with line numbers; the file as a whole still converts.
- Existing conversion budgets/limits (units, expanded bytes, tokens, cancellation, deadlines) still
  apply per conversion; hitting them yields the existing typed truncation/limit outcome with partial
  results, not a failure of earlier records.
- Large files: ingestion in `fm-application` streams the file through the VFS in bounded windows of
  whole lines (never loading the file whole; no `max_source_bytes` rejection for jsonl), feeding
  successive batches to the converter with a running line/byte offset, and is cancellable and
  resumable/idempotent like other indexing (re-index after change replaces stale chunks).
- Tests: converter unit tests (field selection, base64 omission, malformed lines, line provenance,
  budgets, unsorted/mixed records), streaming window boundary tests (record spanning window edge,
  CRLF, missing trailing newline, empty lines), application-level ingestion test with a file larger
  than `max_source_bytes` (use a lowered test budget), and eligibility tests. i18n for any new
  user-visible warning/skip strings in all 8 locales.

## Implementation Notes
- Converter: `crates/fm-semantic-conversion/src/{sniff.rs,converter.rs,formats/}`; follow
  `formats/csv.rs` (row-oriented provenance) as the closest sibling.
- Eligibility/ingestion: `crates/fm-application/src/{semantic_indexing.rs,semantic_ingestion.rs}`.
- Keep the converter path-free and runtime-free (architecture rule); streaming windowing lives in
  the application layer's VFS bridge.
- Read the CLAUDE.md "Semantic worker boundary" notes before touching protocol/DTOs; regenerate
  OpenAPI via `pnpm api:export`/`api:generate` only if DTOs change.

## Agent Notes
- 2026-10-25 claude: task created from user request; not started.
- 2026-10-25 claude: implemented. Converter: `fm-semantic-conversion/src/formats/jsonl.rs` (new `FormatKind::Jsonl`, `Provenance::JsonlRecord{line,byte_offset}`, warnings `SkippedRecord{line,reason}` (first 32 itemized) + `SkippedRecords{count}`, `SkippedRecordReason`). Recognised by media type (`application/x-ndjson` and aliases) or extension `jsonl`/`ndjson` (`sniff::is_jsonl_hint`, extension wins over generic text/json types; container magic still wins in `resolve_format`). JSONL is STREAMED: `BaselineConverter::convert_jsonl` bypasses `materialize`/`max_source_bytes`, reads via `LineReader` (1 MiB window, whole lines reassembled across window edges, running line/byte offsets, CRLF, no trailing newline, blank lines, BOM, lines >16 MiB drained and skipped), memory = window + longest line. Stops reading when an output budget saturates (typed `UnitLimit`/`OutputCharLimit` omissions, earlier records kept); a deadline is a soft stop (`UnreadablePart` omission, partial doc); cancellation -> `Cancelled`; NUL in first line -> `Unsupported`; file with only malformed lines -> `Malformed` (first bad line named). One record = one unit: body = summary + text; title / `channel · date` go once into `section_path` (not body); unit boundary per record so chunks never mix records. Field aliases are case/separator-insensitive tables (title/headline/subject/name, summary/abstract/description, text/content/body/message, date/published/pubDate/published_at/created_at/timestamp incl. epoch s/ms -> YYYY-MM-DD, channel/channel_name/source/feed/site). No known fields -> short (<=256 chars, <=16) string fields as `key: value`; a value identical to the previous record's value for that key is not re-emitted. Data URIs and long base64-looking strings omitted and counted (`UnsupportedFeature` warning). `OptionalConverter` and docling `OcrMyPdfConverter` pass JSONL straight to the baseline (they used to materialize under the source budget). Application: `DocumentConversionService` streams JSONL through `SyncIoBridge` without the size pre-check/limit; `semantic_indexing::media_type` maps jsonl/ndjson (extension first) to `application/x-ndjson`; added to `EligibilityPolicy::curated_defaults`. Frontend citation label `jsonlRecord` -> "Record at line N" (`ragAsk.citationRecord`, all 8 locales). Docs: `docs/semantic-operations.md`. No DTO/OpenAPI change (provenance is an opaque JSON string): `api:export`/`api:generate` leave no diff. Verified: 20 unit tests in `jsonl.rs` + 8 in `tests/jsonl_conversion.rs` + 1 sniff + 3 `document_conversion` + 1 `semantic_indexing` integration (`json_lines_files_are_eligible_and_reindexed_as_ndjson_after_a_change`) + 1 worker ingestion + 1 eligibility + 1 vitest; `cargo nextest run` for conversion/docling/worker/library/application/test-support = 1676 passed; `pnpm run lint` clean; vitest semantic+i18n 291 passed; frontend `tsc --noEmit` clean.
  KNOWN GAPS / follow-ups: (1) Worker indexing (`semantic_indexing::ingest_entry`) still buffers each file via `read_bounded` up to `MAX_SOURCE_BYTES` (64 MiB) and sends it as one `DocumentIngestion`; JSONL files larger than that are still skipped as `Oversized` (and the library catalog rejects > `max_source_bytes_per_document`, 512 MiB default). Streaming a GB-size JSONL to the worker needs a worker transport (like `ingest_pdf_stream`, protobuf change) or per-window sub-documents with occurrence bookkeeping; not done. The no-size-limit, bounded-memory streaming with running offsets is delivered in the converter + host conversion service (summaries/knowledge), and windowing lives in the pure converter's `LineReader` (the builder/budget state must span windows) with the application bridging the VFS stream, not in an application-side batch loop as the task notes suggested. (2) Resumable/idempotent re-index for the >64 MiB case is therefore moot; <=64 MiB files use the existing replace-on-change generation path (tested). (3) New warnings/typed skips are not surfaced in the UI, so no new user-visible strings beyond the citation label. (4) Per-record unit means default `max_units` (20k) caps a document at ~20k records (partial, typed omission).
- 2026-10-25 claude (follow-up, supersedes the KNOWN GAPS above except where restated): worker indexing now has NO file-size limit. Design = per-window sub-documents of whole lines (no protocol change). Converter crate: `JsonlWindower` (pure, chunk-fed; closes windows at 4 MiB or 10,000 lines so a window never reaches the 20k-unit / 8M-char budgets; a line > 16 MiB becomes a one-byte placeholder window so later line numbers/offsets stay exact), `DocumentMetadata::with_jsonl_origin(line, byte_offset)` so each window's record provenance is absolute. Host (`semantic_indexing.rs`): for `application/x-ndjson` pass 1 streams the file (SHA-256 fingerprint, length, window count; no content kept), the library catalog records the observation with `with_streamed_source()` (per-document ceiling skipped, cumulative budgets still enforced; `worker_feed_plan` lifts the 64 MiB buffered limit and the library per-document limit for ndjson), pass 2 re-streams and feeds each window to each workspace as its own worker document (document id `<fingerprint-doc>-w<n>`, occurrence `<occ>@<ws>#w<n>`, shared `source_id`, metadata `jsonl_set/window/windows/line/byte_offset`), then verifies length+digest+window count unchanged (else Unreadable, retried next run, never confirmed) and only then confirms the indexed receipt. Cancellation checked per chunk/window (tested). Worker: `IngestionDocument.jsonl_window`, queue-side metadata validation, per-window budgets, a window with no usable records publishes empty (replaces earlier records), every completed window prunes windows >= its announced count (`occurrence_ids_with_prefix`), so shrinking/editing a file never leaves stale evidence; coverage notes (skipped lines with first line number, empty window, truncation) flow to the host report `coverage_details` and are logged. Replace-on-change/idempotence: new content => new window document ids per window (embedding cache keeps unchanged chunk text cheap); re-running an interrupted pass simply re-feeds. Verified: windower unit tests (any chunking, line/byte limits, blank lines, overlong placeholder, windowed conversion == whole-file conversion), worker tests (3-window feed with absolute line/offset provenance read back from the catalog, pruning when the file shrinks, empty-window replacement, metadata validation), library test (streamed source skips per-document ceiling but not total), application tests with a lowered library per-document limit (4 KiB vs ~40 KiB file) proving full windowing/tiling/replace, a 66 MiB file (> the 64 MiB buffered limit) fully tiled in 16+ windows, and cancellation between windows. `pnpm run lint` clean; nextest for conversion/docling/worker/library/application/test-support: 1688 passed.
  REMAINING (stated plainly): (a) the real IPC worker path was not run end-to-end in this session (the tests use the in-process worker coordinator and the application fake capability); window metadata rides the existing opaque metadata map and 4 MiB windows are far under every protocol limit. (b) Skipped-line counts are not shown in the UI: `SemanticIndexingReport` (with `coverage_details`) is not exposed through any DTO/endpoint today, so surfacing it needs a new DTO + OpenAPI + both hosts + UI; the notes are in the host report and the log. No new user-visible UI strings were added beyond the earlier citation label (8 locales). (c) The file is read twice per index pass (once to hash, once to feed) so a remote provider transfers it twice. (d) Progress for a many-window file is per-window job polling; no aggregate per-file progress event.

