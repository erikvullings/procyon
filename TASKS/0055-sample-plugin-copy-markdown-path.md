# 0055 Sample plugin: Copy Markdown Path

Status: done
Priority: medium
Owner: unassigned
Agent: unassigned
Area: plugins
Depends on: 0054

## Context
`file-manager-coding-agent-spec.md` §20 sample plugin 1 and §36 item 9.

## Acceptance Criteria
- `plugins/sample-copy-markdown-path/` contains a manifest and implementation registering the action
  `sample.copyMarkdownPath`.
- The action is available only when exactly one file or directory is selected.
- It produces `[report.pdf](file:///Users/erik/Documents/report.pdf)` using the entry name as link
  text and a file URI (or a configured relative path) as the target.
- It copies the result to the clipboard using only the `clipboard_write` permission and fails
  visibly if that permission is not granted.
- A success notification is shown.
- The action appears in the command palette and context menu without any core code change.
- Special characters in names are correctly escaped for Markdown and percent-encoded in the URI.
- Tests: action availability rules, generated link for names with spaces/parentheses/Unicode,
  permission denial path.

## Implementation Notes
- This plugin exists to demonstrate action registration, context requirements, clipboard permission,
  selected-entry metadata access and notifications (§20) — keep it minimal and readable.

## Agent Notes
- 2026-08-01 Claude Sonnet 5 (Copilot): Implemented the full `invoke` path this task depended on,
  which did not exist yet after 0054 (only declarative `actions`/`columns`, no execution). Added
  `fm-plugin-api::SelectedEntryContext`, `ActionContribution.requires_single_selection`, and a
  `HostServices.clipboard_write` permission; added `PluginRuntime::invoke_action` in
  `fm-plugin-runtime` with a real `invoke(action_id)` Lua call and permission-gated
  `host.selected_entry_metadata()` / `host.clipboard_write()` host services; extended
  `ActionResultDto` with `clipboardText`; wired `fm-application::FileManagerService` to project
  `requires_single_selection` into `ActionContextRequirements`, dispatch `invoke_action` to enabled
  plugin actions (re-validating context requirements server-side), and publish a success
  notification through the existing event bus on a successful clipboard write.
- Design decision: the caller (frontend) already has the current selection's name and file URI from
  pane state, so it passes them directly as `selectedEntries` invocation parameters rather than the
  backend resolving an `EntryId` back to metadata — there is no id-to-metadata registry in this
  codebase (`EntryId`s are randomized per directory listing) and inventing one was out of scope for
  this task.
- Design decision: the success notification is published by the host automatically on a successful
  `clipboard_write` outcome, rather than via a separate plugin-facing `host.notify()` call, so the
  action's own permission list stays limited to `clipboard_write` and `selected_entry_metadata` as
  required by the acceptance criteria.
- `plugins/sample-copy-markdown-path/` implements `sample.copyMarkdownPath` with
  `requires_single_selection = true`; it Markdown-escapes `\`, `[`, `]` in the entry name and
  percent-encodes the file URI byte-by-byte (correct for UTF-8 multi-byte sequences), preserving
  `/` and `:` as URI structure.
- Known scope boundary: the actual OS/browser clipboard write is a frontend responsibility (a
  server-side Rust process cannot write to a browser client's clipboard — see the existing
  capability comment in `fm-application::service`). The backend only permission-gates the call,
  generates the Markdown text, and returns it as `ActionResultDto.clipboardText` for the frontend to
  copy; this task's own acceptance criteria are all satisfied at the plugin/runtime/application
  layer and covered by tests below.
- Verified: `cargo test` for `fm-plugin-api` (6/6), `fm-plugin-runtime` (14/14, including three new
  tests exercising the real `plugins/sample-copy-markdown-path/plugin.lua` file for the
  single-selection declaration, spaces/parentheses/Unicode link generation, and the permission-
  denial path), `fm-transport-dto` action tests (9/9), and `fm-application` (96/96 lib tests plus
  the existing integration suites), all with zero regressions. `cargo fmt --all -- --check` and
  `cargo clippy --workspace --all-targets` are clean. Regenerated `frontend/openapi/openapi.json`
  and the Orval client for the new `clipboardText` field via `pnpm run api:export` /
  `pnpm run api:generate`.
