# 0286 OCR: opt-in replace original PDF with OCR'd version

Status: open
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
