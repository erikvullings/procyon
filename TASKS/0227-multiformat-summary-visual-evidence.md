# 0227 Multiformat summary visual evidence

Status: done
Priority: high
Subsystem: backend, frontend, semantic
Depends on: 0180, 0184, 0185, 0192

## Context

Document summaries can optionally send bounded DOCX images to a verified vision-capable Ollama
model, but the implementation reopens DOCX through the preview service. This makes image evidence
format-specific even though summaries already depend on the shared semantic conversion pipeline.
Procyon's current deterministic Docling adapter accepts PDFs only and maps `Node::Picture` to its
caption or an omission; `ConvertedDocument` has no visual-attachment output, so Docling and the
baseline converters cannot pass safe image bytes to summaries.

Add a path-free, provider-neutral visual-evidence contract to semantic conversion. Summaries should
consume that contract for supported document formats rather than knowing about DOCX packages,
preview sessions, or converter-specific resources.

## Acceptance Criteria

- `ConvertedDocument` can expose bounded visual attachments with normalized media type,
  deterministic source order, best-available provenance, optional safe caption, and owned bytes.
  Existing text/chunk fingerprints remain path-free and unchanged when only visual extraction is
  added.
- Baseline DOCX, PPTX, EPUB, and supported PDF conversion emit safe local visual attachments where
  image bytes can be proven and decoded. Deterministic Docling PDF conversion preserves or augments
  those PDF visuals rather than discarding them at the adapter boundary.
- Visual extraction never follows external relationships, arbitrary package resources, or remote
  URLs. It accepts only supported raster formats and enforces source, count, aggregate-byte,
  decoded-pixel, and cancellation limits with explicit omission metadata.
- Summary preview and generation use the shared conversion output for indexed and ephemeral files.
  Image inclusion remains an explicit user choice that defaults off and is hidden unless the
  selected default generation model advertises vision and the format supports visual evidence.
- Before generation, selected visuals are resized to at most 1024 px in either dimension and
  constrained by the existing per-image, total-byte, and image-count limits. Included and omitted
  counts are disclosed, and the exact resized bytes participate in the confirmation fingerprint.
- PDF, DOCX, PPTX, and EPUB fixtures prove visual extraction, provenance, deterministic ordering,
  unsupported/external-resource omission, limits, and summary handoff. Text-only and non-vision
  behavior remains unchanged.
- HTTP, Tauri, and mock clients remain equivalent; generated OpenAPI/Orval artifacts are current;
  relevant tests, type checking, and repository lint pass.

## Implementation Notes

- Keep `fm-semantic-conversion` path-free and runtime-free. Converters receive only bounded bytes
  plus trusted metadata; application code retains VFS authority.
- Prefer a backward-compatible constructor that defaults visuals to empty so unrelated converters
  do not require speculative changes.
- Do not claim that current `docling.rs` supplies image bytes: Procyon's pinned deterministic
  adapter is PDF-only and currently receives picture metadata/captions, not reusable raster
  attachments. PDF visual extraction may therefore need a bounded deterministic container pass
  alongside Docling's structural-text mapping.
- Reuse one summary visual preparation path for every format; remove the DOCX-preview dependency
  from summary orchestration once parity is proven.

## Agent Notes

- 2026-09-22 Copilot: Created from user review of the initial DOCX-only summary image support.
  Investigation confirmed `fm-semantic-docling` accepts PDF only and drops uncaptioned
  `Node::Picture` content because `ConvertedDocument` has no image channel. Implementation starts
  by deepening the conversion model and converter fixtures, then replaces the application-layer
  DOCX special case.
- 2026-09-22 Copilot: Added an opt-in, path-free visual-evidence channel to
  `fm-semantic-conversion` and changed summary preparation to consume it for PDF, DOCX, PPTX, EPUB,
  and XLSX files. Package converters follow only declared local relationships and enforce shared
  media, count, byte, pixel, cancellation, and provenance rules; normal semantic indexing remains
  text-only, and visual bytes do not alter embedding input or chunk fingerprints.
- 2026-09-22 Copilot: PDF support retains reusable embedded JPEG/DCT image objects with page
  provenance. It does not yet rasterize complete pages, vector graphics, or non-DCT image streams;
  those sources produce explicit visual omissions rather than overstating support. Preferred
  Docling text output is augmented with baseline PDF visuals, including visual-only fallback for
  image-only PDFs when evidence is requested.
- 2026-09-22 Copilot: Verified 127 conversion unit tests, 6 advanced-converter integration tests,
  9 additional conversion tests, 12 Docling unit tests plus its adapter suite, 682 application
  tests (1 ignored), 6 document-summary dialog tests, frontend type checking, full Rust clippy,
  Biome, and whitespace checks. `pnpm api:check` regenerated the existing artifacts but cannot
  report a clean tree because the branch already contains the intentional unstaged
  `DocumentSummaryDto.full` description change.
