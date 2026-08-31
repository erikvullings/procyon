# 0042 Operation: duplicate

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0040

## Context
`file-manager-coding-agent-spec.md` §16 milestone 2 and §36 item 4 (duplicate is part of the MVP
operation set).

## Acceptance Criteria
- `OperationKind::Duplicate` copies the selection into the same directory with a generated
  non-colliding name.
- Naming scheme is deterministic, documented and collision-safe (e.g. `report.pdf` →
  `report copy.pdf` → `report copy 2.pdf`), preserving the extension and handling dotfiles and
  multi-part extensions (`archive.tar.gz`).
- Works for both files and directory trees, reusing 0039/0040.
- Duplicating a large selection reports aggregate progress as one operation.
- Integration tests: file, directory, existing `copy` names, dotfile, `.tar.gz`, Unicode name,
  read-only source.

## Implementation Notes
- Name generation must be a pure, unit-tested function so the UI can preview the resulting name.
- Platform naming conventions differ (macOS "copy", Windows "- Copy"); pick one, document it, and
  keep it settings-overridable later rather than branching now.

## Agent Notes
- 2026-07-31: Added aggregate multi-selection duplicate through the shared recursive-copy executor.
  The pure naming function handles collisions, dotfiles, full multipart extensions, and Unicode;
  integration coverage includes files, trees, an existing copy, and a read-only source.
