# 0343 Terminal assistant: session root, @ files, / skills and commands

Status: open
Priority: medium
Subsystem: frontend
Depends on: 0342

## Context
The terminal assistant (`# question` at the shell prompt, task 0336) completes `@name` with Tab
against the terminal's current folder only. The user asked (2026-11-23):
- A new session treats the folder it starts in as the trusted root of a repo; `@` completion
  searches its subfolders too. The list may be cached, but files created later (by the agent or
  by commands) must show up.
- Reuse the usual agent conventions: `/` with Tab completion for skills, including skills derived
  from saved notes.
- Commands `/new` (a new session rooted at the current folder), `/clear`, `/compact`, and
  `/model` (switch the chat model among the configured providers).

## Acceptance Criteria
- A session records its root: the terminal's folder at its first question or at `/new`.
- `@` Tab completion matches paths anywhere under the root (relative paths, folders with a
  trailing `/`), from a bounded, cached listing read through the location's provider (local and
  SSH alike). Heavy and hidden folders (`.git`, `node_modules`, `target`, dot folders) are left
  out. The cache is refreshed when the assistant created a file and after a command ran in the
  terminal, so new files complete.
- `# /` Tab completes skill names (folder skills and note skills, as the catalogue lists them) and
  the commands. `# /<skill> rest` asks the question with that skill loaded up front.
- `/new` closes the assistant session and starts one rooted at the current folder; `/clear`
  forgets the conversation but keeps the root; `/compact` replaces the conversation with a
  model-written summary; `/model` lists the chat models of the configured profiles and switches
  the chat role to the one chosen.
- Browser (HTTP) and Tauri behave the same; the mock client supports all of it.
- Tests for completion, cache refresh, skill invocation and each command; lint and tests pass.

## Implementation Notes
- Frontend: `features/terminal/terminal-assist.ts` (completion), `terminal-drawer.ts` (keys,
  session), `FileManagerClient` + adapters.
- Backend: an assistant files listing and skills listing for a root, `/compact` on the session
  (`terminal_assist/session.rs`), skill invocation in the ask request.
- Regenerate `openapi.json` and the Orval client.

## Agent Notes
