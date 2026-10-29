# 0286 OCR: opt-in replace original PDF with OCR'd version

Status: done
Priority: medium
Subsystem: backend, frontend
Depends on: none

## Context
Image-only PDFs are OCR'd by OCRmyPDF on a private temporary copy (opt-in consent, task 0197); the
text is indexed but the user's PDF stays unsearchable elsewhere (Finder/Spotlight, other readers) and
re-indexing after a file change re-runs OCR. User request (2026-10-29): an extra opt-in to replace the
original with the OCR'd version so OCR happens once.

## Decisions (user, 2026-10-29)
- Separate opt-in setting, default off, under Settings > Semantic > OCR. Requires OCR consent.
- Local, writable files only. Skip remote providers (SFTP/FTP/WebDAV/cloud), archive members and
  read-only files; those keep using the temporary OCR copy for the index only.
- Before replacing, verify: the OCR output opens (lopdf) and has the same page count as the source,
  and the source is unchanged since it was read (content hash / size+mtime re-check).
- Then move the original to the system trash / recycle bin and put the OCR'd file at the original path.
  If trashing is unavailable or fails, leave everything untouched and report why.

## Acceptance Criteria
- Setting persisted with the OCR policy; defaults to off; changing it needs no worker restart unless
  the worker hook requires it.
- With the setting on, a remediated local PDF ends with a text layer at its original path and the
  original in the trash; the index reflects the replaced file without re-running OCR.
- Every failed check leaves the original untouched and surfaces a typed reason in the remediation report.
- Worker never gains write authority over user files: OCR output reaches the host through a
  host-private retention directory (config/env), host verifies and replaces.
- Service + DTO + Tauri command + client implemented in mock, http (unavailable/read-only) and tauri
  adapters; settings UI in all 8 locales; `pnpm api:export`/`api:generate`/`api:check` clean.
- Tests use temp roots only; no destructive test outside them.

## Implementation Notes
- Stage in separate commits: (1) policy field + service/DTO/command/client plumbing, (2) converter
  retain-output hook (`OcrMyPdfConfiguration`) + worker env, (3) host replace step with verification
  and trash, (4) settings UI + i18n + docs.
- Relevant: `crates/fm-semantic-docling/src/lib.rs` (`OcrMyPdfConverter::run_ocr`: runs
  `--output-type pdf --redo-ocr --optimize 0`), `crates/fm-application/src/semantic_ocr.rs`
  (`OcrPolicyStore`, `PersistedOcrPolicy`), `semantic_ocr/remediation.rs` and `coordinator.rs`
  (per-file jobs), `crates/fm-application/src/service.rs` (`set_semantic_ocr_consent`),
  `apps/fm-desktop/src-tauri/src/commands.rs`, `frontend/src/features/settings/semantic-ocr-management.ts`.
- Read `docs/semantic-operations.md` OCR section; authority boundary rule: Procyon keeps filesystem
  authority, the worker only receives bytes.

## Agent Notes
- 2026-10-29: Created from the discussion on task 0285 (probe showed 3 of 22 corpus PDFs have no text
  layer). Nothing implemented yet.
- 2026-10-29 Claude (4 commits): (1) `replaceOriginal` in `PersistedOcrPolicy` (additive serde default,
  no schema bump; forced off when consent is withdrawn), `set_semantic_ocr_replace_original` service,
  `SetSemanticOcrReplaceOriginalRequestDto` + Tauri command + client in mock/tauri/http(unavailable);
  the OCR DTOs are hand-authored, so `openapi.json`/Orval output is unchanged. Changing the opt-in
  restarts the worker (retention dir is fixed at launch). (2) `OcrMyPdfConfiguration::with_retention_directory`
  (+ `PROCYON_OCRMYPDF_RETAIN_DIR`, validated absolute existing dir): worker writes `<sha256(OCR input)>.pdf`
  atomically; command line unchanged. Host sets/clears the env at launch for managed and developer
  workers; dir is `<semantic data>/extracted/ocr-retention`, mode 0700 on unix. (3)
  `semantic_ocr/replace.rs`: snapshot (size+mtime, regular file, not read-only) before the worker
  reads the file; after success verify content digest finds retained output, lopdf opens both and page
  counts match, re-check snapshot right before swap, stage beside original, trash via `PlatformAdapter`
  (needs TRASH capability), rename in place, restore original if the rename fails; typed
  `OcrReplacementSkip` per file in job report; re-ingest after replace; retained files purged at job end.
  (4) Settings checkbox (disabled without consent), per-file replaced/kept line, 8 locales, docs.
- Verified: fm-application (1335 tests incl. replace unit tests with fake trash in temp roots and
  service-level tests for replaced / not opted in / page-count mismatch), fm-semantic-docling and
  fm-semantic-worker OCR tests, vitest + tsc + biome, clippy `-D warnings`.
- Not verified: a real OCRmyPDF run end-to-end, the real macOS/Windows/Linux trash adapters in this
  flow (only a fake folder trash; macOS adapter has its own tests), Windows and Linux at all, the
  `semantic-gemma` developer path beyond compiling. Known limits: retained output from ordinary
  (non-remediation) OCR during indexing accumulates until the next remediation job ends; directory
  mtime/permission failures on the parent surface as `replaceFailed`; no cap on source size read for
  verification beyond the existing PDF ingest limit.
- 2026-10-29 Real-flow check (macOS, real OCRmyPDF 17 at /opt/homebrew/bin, real `MacosPlatformAdapter`
  trash) on COPIES of 4 PDFs from the Leiden corpus, via the ignored test
  `real_ocrmypdf_replaces_scanned_pdf_copies` (`PROCYON_OCR_REPLACE_TEST_DIR=<dir with docs/>`, run with
  `cargo test -p fm-application --test semantic_indexing -- --ignored real_ocrmypdf`). Result: the 3
  image-only PDFs (early attachment 6p, peer victimization 9p, developmental assets 9p) were all
  `Succeeded / Replaced`; afterwards pdftotext finds 29.7k / 37.3k / 14.0k chars and the page counts
  are unchanged. Loeber (16p, 508 chars, partly scanned) is not reported as requiring OCR (it has a
  text layer), so it was correctly left untouched. The first run showed `RetainedOutputMissing`, which
  was a harness bug (default 30 s conversion budget starved OCR; the worker uses 4 min) - no product
  bug found. The Trash contents could not be listed (~/.Trash is permission-protected for the agent),
  so the trashed original is inferred from the adapter call succeeding. Windows/Linux still untested.

