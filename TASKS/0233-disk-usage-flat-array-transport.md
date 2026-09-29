# 0233 Disk usage: flat-array tree transport

Status: done
Priority: low
Subsystem: backend
Depends on: 0230, 0231, 0232

## Context
BlitzTree hands its tree to the UI as parallel flat arrays (parents, sizes, flags, child offsets,
name blob). Our recursive `DiskUsageNodeDto` JSON repeats a full `Location` per node, which is why
the response is capped at depth 4 and 2,048 children per directory.

## Acceptance Criteria
- A compact flat representation (per node: parent index, name, kind, logical/physical bytes,
  collapsed; locations derived from root + names) is used for disk-usage responses and progress
  events across HTTP, Tauri and mock adapters, with OpenAPI/Orval regenerated.
- The response depth cap can be raised without regressing payload size for typical scans.

## Agent Notes
- Wire form: `DiskUsageTreeDto` (in `fm-transport-dto`, registered in the OpenAPI document for the
  generated TS type), mirrored by `DiskUsageTreePayload` in `fm-events`. It is a pre-order set of
  parallel arrays: `parents`, `names`, `flags` (kind in bits 0–1, collapsed in bit 2),
  `logicalBytes` and `physicalBytes`, plus `providerId`/`rootUri`. A sparse `uriOverrides` map
  (decimal index → URI) covers nodes whose URI isn't `parent + "/" + name`: percent-encoded names
  and the synthetic "Small files (N)" aggregate, which shares its parent's location.
- `diskUsage.progress` now carries `tree` instead of the recursive `root`. The backend still builds
  `DiskUsageNodeDto` internally and flattens it once per emitted event (`event_tree`). The
  frontend decodes the tree back to nested nodes in `decodeDiskUsageTree` (`models/disk-usage.ts`)
  inside the event handler, so the view, merge and expansion code are unchanged. Inconsistent
  trees are ignored with a warning. The mock encodes with `encodeDiskUsageTree`. The HTTP (SSE)
  and Tauri adapters pass event payloads through unchanged, so all three adapters share one codec.
- Measured on real scans (JSON bytes): at the old depth 4, `~/Library` (19k nodes) was 5.1 MB
  nested versus 0.92 MB flat. Depth 5 flat is 4.65 MB (77k nodes), so `MAX_RESPONSE_DEPTH` was
  raised from 4 to 5 while staying below the previous payload. Depth 6 grew to 564k nodes / 73 MB,
  so it was not pursued. A procyon worktree at depth 5 is 0.37 MB.
- Tests: the Rust round trip, rejection of inconsistent arrays, and a "flat under a third of nested"
  size test; the TS codec round trip, trailing-slash root and rejection; and the handler ignoring an
  inconsistent tree. The existing service and server tests decode the event tree.
