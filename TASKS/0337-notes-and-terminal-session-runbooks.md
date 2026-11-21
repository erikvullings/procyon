# 0337 Notes: terminal sessions to runbooks, searchable by the assistant

Status: done
Priority: medium
Subsystem: backend, frontend, settings, semantic
Depends on: 0336

## Context
The user does some tasks rarely and forgets the steps (replacing an nginx certificate, creating a
CSR). The terminal assistant (0336) already has the whole session: questions, commands, exit codes,
output, host and folder. Turn it into a reusable note, and let the assistant (and later other LLMs)
read those notes again ("how did I do this last time?").

Decision (with the user, 2026-11-20): a **thin** notes feature, not a PKM.
- Notes are plain Markdown files in one notes folder. Obsidian, Zettlr or any editor can open the
  same folder; Procyon does not compete with them.
- Procyon adds what only it can do: capture from terminal sessions, semantic indexing of the notes,
  and assistant tools to search, read and propose updates.
- Out of scope: graph view, backlinks panel, daily notes, templates UI, sync, a notes plugin API.
  Obsidian covers these on the same files.

## Design

### `NotesService` (new capability module, `crates/fm-application/src/notes/`)
`FileManagerService` only delegates (see CLAUDE.md "Application capability services"). The service
owns:
- **Notes root**: resolve, validate and persist the notes folder (a VFS location URI).
- **Capture**: transcript → draft note (LLM distillation + redaction) → user review → save.
- **Search**: semantic when available, text search fallback (below).
- **Read / propose update**: bounded read of one note; an update is a proposed new text, saved only
  after the user approves it.

It depends on the semantic library/query capability, the LLM chat generator and the terminal-assist
session (read-only access to the transcript), all passed in explicitly. It must not depend on
`FileManagerService`. Writes go through the existing file-save/create path (`FileEditorService` or
the create-file operation), never direct `std::fs` writes, so conflict handling and "never silently
overwrite" hold (spec rules 6 and 8).

### Notes folder
- **Suggested default**: `~/Documents/Procyon Notes` on desktop (via the platform documents dir,
  e.g. `dirs::document_dir()`; fall back to `~/Procyon Notes` when there is none). On `fm-server` no
  default is guessed: the user must choose a location inside the server's allowed roots.
- **First use** (first "Save as note", or Settings → Notes → "Set up notes"): a setup dialog shows
  the suggested folder, editable and with a folder picker. The user can pick an existing Obsidian
  vault. The folder is created if missing (after confirmation).
- Stored as a setting (`notes.root`, location URI) through the settings migration machinery.
  Changeable later in Settings → Notes; changing it never moves or deletes files.
- Any writable VFS location is allowed. Semantic indexing only applies where the semantic library
  supports the location (local in practice); otherwise search uses the text fallback.

### Automatic semantic indexing
- The setup dialog has an "Index notes for semantic search" checkbox: **checked by default** and
  shown only when the semantic capability is installed and enabled for this host
  (`semantic_library_capabilities` supports `Enrol`).
- Consent model stays intact: setup calls `preview_semantic_enrolment` for the notes folder
  (recursive) and shows the disclosure inline in the same dialog. Confirming setup with the box
  checked calls `confirm_semantic_enrolment`. No silent enrolment and no second dialog.
- If semantic is enabled later, Settings → Notes offers "Index notes" (same preview/confirm).
- Excluding the folder from the semantic library (existing flow) is respected; notes keep working
  through the text fallback.
- New or updated notes are picked up by the library's normal reconcile/watch path; no special
  ingestion code in `NotesService`.

### Note format
```markdown
---
title: Replace nginx TLS certificate
created: 2026-11-20T14:03:00Z
updated: 2026-11-20T14:03:00Z
tags: [nginx, tls, certificate]
hosts: [web-01]
shell: bash
source: terminal-session
---
# Replace nginx TLS certificate

## Goal
## Prerequisites
## Steps
1. ... with fenced `bash` blocks holding the commands that worked
## Verify
## Pitfalls
(failed attempts and how they were fixed, taken from non-zero exit codes in the session)
```
- File name: slugified title, `.md`; on a clash, ask (never overwrite) or suggest a `-2` suffix.
- Subfolders are allowed and preserved; the default target is the root.
- `[[wikilinks]]` are allowed in the body (Obsidian-compatible), but this task builds no link UI.

### Capture: "Save session as note" (terminal assistant)
1. Command in the terminal assistant bar plus the `#note` / "Save as note" action (exact UI follows
   0336's slim bar; add a command palette action as well).
2. Input: the session messages, command blocks (command, exit code, bounded output), environment
   (OS, shell, host, cwd). Already redacted by 0336's `redact.rs`; run redaction **again** on the
   generated draft, because the model may echo or rebuild secrets.
3. The LLM (configured chat role, 0322; cloud only with consent) returns the note in the format
   above. Prompt: keep the commands that worked, generalize host-specific values as placeholders
   (`<domain>`, `<cert-path>`) where obvious, list failed attempts under Pitfalls.
4. Review dialog: editable title, tags, target folder/file name, Markdown body with preview.
   Redaction hits are highlighted with a warning ("3 values were replaced by `[REDACTED]`; check
   them"). Nothing is written before **Save**.
5. Save writes the file via the operations/editor path; a toast offers "Open note".
- Also allow "Save selection as note" for a range of blocks, and appending to an existing note
  ("Add to note…") as a proposed update (diff shown before save).

### `search_notes` and other assistant tools (extends 0336's tool set)
| Tool | Approval | Behaviour |
| --- | --- | --- |
| `search_notes { query, limit? }` | auto | Ranked hits: path, title, tags, best snippet. |
| `read_note { path }` | auto | Bounded read of one note inside the notes root. |
| `propose_note_update { path, content }` / `propose_note { title, content }` | user | Shown as a diff / new-note review; saved only on approval. |

Search strategy, in order, chosen per call (the result says which one was used):
1. **Semantic**: if the notes folder is enrolled and the library is ready, call `semantic_query`
   scoped to the notes folder (include-folder scope, see 0241). Merge chunk hits per file.
2. **Text fallback**: otherwise (not installed, not enrolled, paused, worker unavailable, remote
   location, or the semantic call fails): an in-process, grep-equivalent search over the notes
   root in Rust. Tokenize the query; score by matches in title/frontmatter tags > headings > body;
   case-insensitive; `.md`/`.markdown`/`.txt` only; bounded file count, size and time.
   Reuse/extract the search walker from `terminal_assist/tools.rs` (`Sandbox::search`) or the
   existing content-search code in `SearchComparisonCoordinator`, not a second copy.
   Do **not** shell out to `grep`/`rg`: works the same on Windows, over VFS providers, in both
   hosts, and avoids command injection. (Using `rg` when present as a speed-up is optional and
   needs the same result shape; skip it unless measurements show a need.)
- Notes tools work for local and SSH terminal tabs alike: they read the notes root, not the
  terminal's working folder, so 0336's "read-only tools only inside the cwd" sandbox does not
  apply. They are the one exception and are limited to the notes root (canonicalised, no symlinks
  out, secret-named files skipped, output redacted).
- System prompt hint: "Before answering how-to questions, search the user's notes."

### Host parity
- Notes root setup, Settings → Notes, the review dialog and `search_notes`/`read_note` work in both
  hosts (HTTP + Tauri); expose them as REST endpoints and Tauri commands over the same
  `NotesService` methods. Regenerate the client (`pnpm api:export && pnpm api:generate`).
- "Save session as note" follows the terminal (Tauri-only, like 0333/0336). State this explicitly;
  it is not a parity bug.
- Mock client: in-memory notes plus fake search so the UI is testable with `pnpm dev:mock`.

## Acceptance Criteria
- First "Save as note" opens setup with a suggested default folder that the user can change; the
  choice persists in settings and can be changed later without moving files.
- With semantic installed, setup shows the enrolment disclosure and enrols the notes folder on
  confirm (box checked by default); with it unchecked or semantic absent, no enrolment happens.
- From a terminal session, "Save as note" produces a reviewable Markdown runbook with frontmatter,
  steps with the working commands and a Pitfalls section; nothing is written before Save; an
  existing file is never overwritten silently.
- Secrets in the session (private key blocks, `export TOKEN=...`, `Authorization:` headers,
  password flags) never appear in the saved note unless the user types them back in; redaction hits
  are shown in the review.
- `search_notes` returns semantic hits when the folder is enrolled and ready, and text-fallback hits
  otherwise; the result names the strategy used. Both paths return the same DTO shape.
- In a new terminal session, asking "how do I replace the nginx certificate" makes the assistant
  call `search_notes`, then `read_note`, and answer from the note.
- `propose_note_update` shows a diff and writes only after approval.
- Notes tools cannot read outside the notes root (path traversal and symlink tests).
- Browser host: notes setup, settings and search work; Tauri host: all of it including capture.
- `pnpm run lint` clean; Rust and frontend tests added.

## Implementation Notes
- Backend: `crates/fm-application/src/notes/` (`mod.rs` service, `root.rs` setup/validation,
  `capture.rs` prompt + draft + redaction, `format.rs` frontmatter parse/emit, `search.rs`
  semantic/text strategies behind a small `NoteSearch` trait, `tools.rs` assistant tool adapters).
  Wire the tools into `terminal_assist/protocol.rs`/`tools.rs` via the service, not by copying
  logic. DTOs in the API crate; domain types stay separate from DTOs.
- Redaction: reuse `terminal_assist/redact.rs` (move it to a shared module if both need it).
- Settings: new `notes` slice (`root`, `indexed` is derived from library state, not stored).
- Frontend: Settings → Notes section; setup dialog (default folder, picker, semantic checkbox +
  inline disclosure); review dialog (Markdown editor + preview, redaction warnings, diff view for
  updates); "Save as note" in the terminal assistant bar and command palette. i18n in all locales.
  Meiosis state slice for notes setup/review; no component-local shared state.
- Tests: frontmatter round-trip; slug/clash handling; redaction on generated drafts; capture with a
  fake `ChatGenerator`; search strategy selection (enrolled+ready → semantic; each failure mode →
  text) with a fake semantic capability; text-scoring order; sandbox traversal/symlink refusal;
  setup flow with and without semantic capability; frontend dialog tests (jsdom).
- Later, separate tasks (not this one): exposing notes over MCP for external LLMs; wikilink
  navigation/preview in the viewer; "related notes" via 0306 semantic neighbours.

## Agent Notes
- Created 2026-11-20 from a design discussion with the user (thin notes on plain Markdown files
  rather than an integrated PKM; user asked for a dedicated service, a suggested but overridable
  default folder, automatic semantic indexing when available, and a text fallback for search).
- 2026-11-20 Claude: implemented (backend, both hosts, frontend). Summary and deviations:
  - Backend `crates/fm-application/src/notes/`: `format.rs` (front matter subset, slugs with
    folded diacritics), `store.rs` (VFS-only access, so local and remote roots behave alike; no
    `..`/absolute paths, no symlinks followed, hidden and credential folders skipped, secret-looking
    names refused, bounded walk/read; `path_of` maps a location back and re-checks it),
    `search.rs` (semantic first, text fallback; also falls back when the index returns nothing or
    only hits outside the folder; text scoring: title/file name > tag > heading > body, weighted by
    the share of terms found, ties by path), `semantic.rs` (`KnowledgeNotesIndex`: Structured
    Knowledge Search with `CurrentFolder` scope, evidence resolved to locations; consent stays with
    the knowledge service, any failure means text fallback), `capture.rs` (session + blocks →
    redacted prompt → runbook; reply parsed from `Title:`/`Tags:` lines, front matter or a fenced
    reply; redacted again; redaction marks counted), `tools.rs` (`AssistantNotes` implements the
    new `terminal_assist::NotesAccess`), `mod.rs` (`NotesService`, `notes_error`).
  - Notes root lives in `Settings.notes_root` (backend-owned like `folder_views`: kept on
    `update_settings`, not in the settings DTO). Desktop suggestion: `~/Documents/Procyon Notes`
    (home fallback); server suggests none.
  - Writes never happen in `NotesService`: new notes use the `createFile` operation (conflict
    policy `ask`), updates use `saveEditableFile` with the revision from `read_note`, a missing
    folder is created with the `createDirectory` operation.
  - Assistant: `search_notes`, `read_note`, `propose_note`, `propose_note_update` in the text tool
    protocol; a proposal ends the turn like `run_command` (`noteProposal` on the reply).
    `TerminalAssistRequestDto.workspaceId` scopes semantic consent. The facade's
    `terminal_assist[_streaming]` now take the host's `SemanticAccessContext`. Notes tools read the
    notes folder for SSH tabs too.
  - Hosts: HTTP `GET /api/v1/notes/setup`, `POST /api/v1/notes/{root,search,read}`; the server
    checks the root against accessible roots on set and again on search/read. Tauri commands
    `get_notes_setup`, `set_notes_root`, `search_notes`, `read_note`, `draft_note` (draft is
    desktop-only, like the terminal; HTTP and mock reject it with `desktopHostRequired`).
    New rejection reasons `notesNotConfigured`, `notesRootUnavailable`, `notesPathUnavailable`,
    `notesNotText` (translated in all locales).
  - Frontend `features/notes/`: state + pure helpers, controller (music-duplicates pattern: a
    `shellState.notesState` slice and a controller, not a new store), dialogs (setup, consent
    step, drafting, review with Edit/Preview/Changes and redaction warning), Settings section
    (inside Files & operations), palette actions `client.setUpNotes` and
    `client.saveTerminalNote` (desktop), "Save as note" button in the terminal assistant bar.
    `MockNotesClient` serves two sample notes for `pnpm dev:mock`.
  - Verified: 35 `notes::` Rust tests (`cargo test -p fm-application --lib notes::`), 4 new
    `terminal_assist` tests (80 in that module), 1 HTTP integration test
    (`cargo nextest run -p fm-server --test notes_routes`, allow and deny paths), 22 frontend
    tests in `src/features/notes/` and 2 new drawer tests; frontend `tsc --noEmit` clean; workspace
    clippy `-D warnings` and `cargo fmt --check` clean; `pnpm api:export`/`api:generate`
    regenerated; production `vite build` succeeds.
  - Deviation: the semantic consent is the second step of the notes flow (the setup dialog is
    replaced by the existing `SemanticFolderEnrolmentPrompt`), not inside the first dialog: the
    enrolment preview needs the folder to exist, and it may only be created after the person
    confirms the folder.
  - Not built (follow-ups): "Save selection as note" for a range of blocks; a dedicated "Add to
    note…" (the assistant's `propose_note_update` covers updates); a `#note` prompt shortcut; new
    notes in a subfolder (the file name may not contain folders; existing notes in subfolders are
    read, searched and updated). The drawer sends neither shell name nor host for drafts, so the
    `hosts`/`shell` front-matter keys stay empty from the UI path (the backend supports them).
  - Not verified: the Tauri app, a live LLM (drafting quality, whether models call
    `search_notes` unprompted), the semantic path against a real worker (only a fake index is
    tested), and the UI visually (port 5180 was taken and the browser pane refused navigation;
    covered by jsdom tests and a production build only). Mock saves do not appear in mock search.
  - Full Rust suite after the change: `cargo nextest run --workspace` 3950 passed, 21 skipped.
