# 0343 Terminal assistant: session root, @ files, / skills and commands

Status: done
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
- The terminal is desktop-only (`TerminalClient` is Tauri IPC; the browser host has no PTY), so
  the new calls are Tauri commands; the client methods are optional so other clients degrade to
  the old one-folder completion.
- Tests for completion, cache refresh, skill invocation and each command; lint and tests pass.

## Implementation Notes
- Frontend: `features/terminal/terminal-assist.ts` (completion), `terminal-drawer.ts` (keys,
  session), `FileManagerClient` + adapters.
- Backend: an assistant files listing and skills listing for a root, `/compact` on the session
  (`terminal_assist/session.rs`), skill invocation in the ask request.
- Regenerate `openapi.json` and the Orval client.

## Agent Notes
- 2026-11-23 Claude: Done.
  - Backend (`fm-application/src/terminal_assist/commands.rs`): `list_files` (breadth first
    through the location's provider, so SSH works; 5000 paths, depth 8; dot entries, links and
    `node_modules`/`target`/`dist`/`build`/`out`/`__pycache__`/`venv`/`vendor`/`coverage` left
    out), `list_skills` (project context skills plus `NotesAccess::list_skills`, a new default
    trait method; `AssistantNotes` lists up to 200 notes by name), `invoked_skill` (folder skill
    body or note text, 16 KiB, redacted, added to the person's context; an unknown name says so),
    `compact` (model summary replaces the session: `Sessions::replace_with_summary`).
  - `TerminalAssistRequestDto` gains `root` and `skill`; `tools_folder` uses the root while the
    working folder lies inside it (folder-boundary check), for tools, mentions, skills and
    instructions. Output files (`write_file`) still go to the working folder.
  - Tauri: `terminal_assist_files`, `terminal_assist_skills`, `terminal_assist_compact`.
  - Frontend: `slash-commands.ts` (parse, complete, `/model` choices), `completeMentionInTree`,
    drawer session root (`assistRoot`, set at the first question or `/new`), completion caches
    marked stale after each reply and each shell command, `/new` `/clear` `/compact` `/model`
    `/help`, `TerminalModels` wired in `app-shell.ts` (profiles, discovered models, `setLlmRole`
    keeping `allowCloud`). Strings in all 8 languages.
  - Verified: Rust 6 new tests (files listing incl. a new file, skill invocation, root rule,
    boundary check, compact); frontend 4070 tests incl. new slash/mention/drawer tests; tsc and
    Biome clean.
  - Not verified in the running desktop app or over a real SSH connection.
