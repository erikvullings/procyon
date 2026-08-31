# 0121 Extract File Editor Service

Status: done
Priority: medium
Subsystem: backend
Depends on: 0119

## Context

`FileManagerService` contains `load_editable_file()` (~80 lines), `save_editable_file()` (~130 lines), and `content_revision()` helper for optimistic-concurrency-based file editing. This is a self-contained capability with its own invariants (MAX_EDITABLE_FILE_BYTES, revision-based conflict detection, atomic sibling-temp-save pattern), but requires bootstrapping the full service to test.

## Acceptance Criteria
- `FileEditorService` module with `load()` and `save()` methods
- Owns `MAX_EDITABLE_FILE_BYTES` constant and `content_revision()` helper
- Atomic sibling-temp-save logic contained within the module
- Revision conflict detection testable in isolation
- Binary file rejection, UTF-8 validation, size-cap enforcement all testable without full service
- `FileManagerService` delegates to a single `editor` field
- Zero behavioural changes

## Implementation Notes
- Needs: `ProviderRegistry`, `audit_log_path` (for overwritten conflict logging)
- The module is small (~250 lines) but self-contained — the tests are the value-add
- Existing integration tests in `crates/fm-application/tests/` may already cover some of this; verify overlap

## Agent Notes

Completed: FileEditorService extracted to `file_editor.rs` (~250 lines) with full isolated test suite (6 tests). FileManagerService delegates to `editor` field via `self.editor.load()` and `self.editor.save()`. All 10 editable-file tests pass (6 module + 4 service integration).