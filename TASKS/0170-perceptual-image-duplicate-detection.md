# 0170 Perceptual image duplicate detection

Status: open
Priority: low
Subsystem: backend, frontend, checksum
Depends on: 0077, 0134

## Context

Exact checksum duplicate detection cannot find visually equivalent images that were resized,
recompressed, lightly cropped, or stripped of metadata. An opt-in perceptual comparison mode would
help photo-library cleanup while clearly distinguishing similarity from byte identity.

## Acceptance Criteria

- Duplicate detection offers a separate, explicitly labelled Similar images mode for supported image
  formats and retains exact checksum mode unchanged.
- The backend computes a documented perceptual fingerprint from bounded decoded images, caches it by
  source revision, and groups candidates using a configurable similarity threshold.
- Results show a representative image, thumbnails, dimensions, file sizes, similarity score, and
  enough location context to make a deletion decision.
- The UI never labels perceptual matches as exact duplicates and does not automatically select files
  for deletion.
- Scanning is cancellable, memory/concurrency bounded, and resilient to corrupt images and unsupported
  formats.
- Any delete/move action from results uses the normal operation engine and confirmation behavior.
- Tests cover resize/recompression similarity, unrelated images, rotations if supported, corrupt
  input, cache invalidation, threshold boundaries, cancellation, and large candidate sets.

## Implementation Notes

- Reuse thumbnail decode/orientation handling where possible, but keep perceptual hashes separate from
  cryptographic checksum identities.
- Start with one documented algorithm and measured fixtures before exposing algorithm choices.
- Candidate bucketing must avoid an all-pairs comparison for large libraries.

## Agent Notes

- 2026-08-28: Created from the product feature review as a useful extension of exact duplicate
  detection, with conservative deletion UX.
