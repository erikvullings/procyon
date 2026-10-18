# 0247 Change Gemma library media consent

Status: done
Priority: high
Subsystem: semantic, backend, frontend
Depends on: 0239

## Context

An existing EmbeddingGemma 2 library currently fixes both its output dimension
and its image/audio/video selections at creation. Users need to add or revoke
media consent without losing the library's existing text index. E5 remains the
default; Gemma remains opt-in. The v0.6.0 release is risk-accepted and is not
five-gate qualified.

## Acceptance Criteria

- Keep the Gemma model revision, output dimension, and vector space fixed.
  Allow changing image, audio, and video consent independently at any time;
  text remains indexed. Migrate existing libraries without discarding vectors.
- Persist changes through the versioned semantic-library policy. Enabling a
  medium indexes previously skipped files incrementally. Disabling requires
  explicit confirmation, immediately revokes query/feed authorization, and
  durably removes that medium's vectors and evidence through resumable,
  idempotent cleanup. Restart and retry must preserve the revocation.
- Continue gating per-folder PDF page-image indexing by image consent.
  Enforce the new consent in the worker as well as host eligibility.
- Present current selections and a change control in Settings; explain that
  only dimension is locked. Keep HTTP, Tauri, and mock adapters equivalent and
  regenerate the OpenAPI/Orval artifacts.
- Cover incremental addition, confirmed removal, restart/idempotency, and the
  user-facing controls with focused tests.

## Agent Notes

- 2026-10-18 Copilot: This implementation remains complete. The release owner
  accepts the v0.6.0 release risks separately; the opt-in model choice and
  immutable per-library media consent do not imply that the five-gate release
  qualification passed.
