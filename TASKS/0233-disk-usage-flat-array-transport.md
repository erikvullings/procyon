# 0233 Disk usage: flat-array tree transport

Status: open
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
