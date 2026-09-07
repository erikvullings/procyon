# 0197 Production OCRmyPDF discovery and consent

Status: open
Priority: medium
Subsystem: frontend, backend
Depends on: 0193, 0196

## Context

The OCRmyPDF fallback currently works only through an explicit development environment variable.
For release users it must remain optional, discover a user-installed executable safely, explain
what will run, and expose background remediation for files already reported as requiring OCR.
OCR support must not block the initial semantic release.

## Acceptance Criteria

- Settings reports whether a supported OCRmyPDF executable is available, its resolved version, and
  why an installation is rejected; arbitrary user-supplied command strings are never executed.
- Users explicitly enable or disable OCR remediation and can start it for one file, selected files,
  one enrolled root, or all currently reported OCR-required files.
- OCR runs as bounded cancellable background work, reports queued/running/completed/failed state,
  and automatically reconverts and ingests successful outputs.
- Existing private temporary storage, sanitized environment, process-group termination, deadlines,
  output limits, and no-source-overwrite guarantees remain enforced in release builds.
- The UI distinguishes missing executable, unsupported version, OCR failure, post-OCR no-text, and
  successful ingestion, with keyboard and screen-reader coverage.
- Tests cover executable discovery, consent persistence, queueing, cancellation, restart recovery,
  failures, successful re-ingestion, and disabled behavior.

## Implementation Notes

- Reuse the 0193 `OcrMyPdfConverter`; this task owns production discovery, policy, orchestration,
  and UX rather than another converter.
- Do not bundle OCRmyPDF silently. Platform installation guidance may link to trusted documentation.
- 0198 does not depend on this task: semantic search can ship without automatic OCR remediation.

## Agent Notes

- 2026-09-07 Copilot: Tracked separately because OCRmyPDF is a user-installed optional executable,
  not one of the signed semantic component artifacts required for the first production release.
