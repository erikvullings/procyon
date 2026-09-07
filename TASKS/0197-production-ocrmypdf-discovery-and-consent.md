# 0197 Production OCRmyPDF discovery and consent

Status: done
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
- 2026-09-09 Copilot: Started production discovery and remediation orchestration after 0196.
  Production discovery is isolated in `fm-semantic-docling`; application policy and queueing will
  reuse the existing semantic indexing capability rather than adding OCR branches to search or Ask.
- 2026-09-07 Copilot: Completed production desktop OCR remediation. Added bounded safe executable
  discovery and version rejection, versioned default-off consent, a durable single-consumer queue
  with restart recovery and cancellation, backend-authoritative expansion for one file, selected
  files, one enrolled root, and all reported files, worker retirement on consent changes, and
  selected-file re-ingestion that clears only successful/no-longer-eligible OCR reports. Added a
  hand-authored shared DTO, Tauri commands and registration, an explicitly unavailable HTTP
  adapter, deterministic mock behavior, and keyboard/screen-reader-tested Settings controls with
  complete locale keys. Verified the full affected Rust crate suites, all 1,873 frontend tests,
  TypeScript typechecking, workspace Clippy/Biome lint, and unchanged generated OpenAPI artifacts.
  Native Windows intentionally remains unavailable and directs users to WSL; the real external
  OCRmyPDF smoke test remains opt-in while controlled executable tests cover the release contract.
