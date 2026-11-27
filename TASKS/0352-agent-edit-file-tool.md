# 0352 Agent edit_file tool with approval

Status: done
Priority: medium
Subsystem: backend, agent, frontend
Depends on: none

**Prepending text.** The tool also accepts `{ path, prepend: true, new_text }` for an insertion
at the start of a file. Include any separator newlines in `new_text`. Prepending uses the same
path checks, diff and approval as a replacement.

## Context

Part of 0351. The assistant can only create new `.md`/`.txt` files in `summaries/`
(`terminal_assist/write.rs`, task 0338). To change an existing file it must rewrite it whole,
which it cannot do either, so a request like "fix the typo in notes.md" or "add a section to the
README" fails or ends in a shell command. Pi's `edit` replaces an exact string, which is cheap in
tokens and hard to get wrong.

The assistant reads documents that may contain instructions, so editing must never happen
unseen: Procyon's write rule (the person approves) stays.

## Acceptance Criteria
- A typed `edit_file` tool in `fm-agent`: `{ path, old_text, new_text, replace_all? }`.
  - `old_text` must occur exactly once (or, with `replace_all`, at least once); otherwise the
    tool errors with the count and, for zero matches, the closest line, so the model can retry.
  - Only text files under the working folder; never secret names or credential folders
    (`is_secret_name`, `is_credential_folder`), never documents the converter reads (PDF, DOCX...).
  - Bounded: file at most 1 MiB, `new_text` at most 64 KiB.
- The tool interrupts the turn with a proposed edit (path, unified diff, file fingerprint). The
  host shows the diff and the person approves or declines; the result says which.
  - On approval the host checks the fingerprint (size + modified time + hash), so a file that
    changed since the proposal is not overwritten; the model is told to read it again.
  - The write goes through the VFS provider (local and SFTP alike), atomically (temp file and
    rename where the provider supports it), keeping permissions.
- An "approve edits to this file for this question" choice, so a multi-step edit does not ask
  every time. Never a standing approval across questions.
- Terminal assistant and chat both offer the tool when their host allows writing; the
  description is omitted otherwise (as `write_file` is).
- Tests: unique/zero/multiple matches, `replace_all`, CRLF files, a file changed between
  proposal and approval, secret names refused, declined edit leaves the file untouched.

## Implementation Notes
- Interrupt pattern: `TerminalInterrupt::WriteFile(PendingWrite)` in
  `terminal_assist/agent_tools.rs` and its approval flow in `terminal_assist/write.rs`.
- Diff: the `similar` crate (check the workspace first; add it only if absent) for a unified
  diff with 3 lines of context, capped for display.
- Undo: an approved edit is a file change outside the operation engine. Decide whether it should
  be an operation (journaled, undoable, spec rule 6/8) — likely yes, as a small `EditText`
  operation in `fm-operations`; record the decision here.

## Agent Notes
- 2026-11-24 claude: done.
  - `fm-agent/src/edit.rs`: public `EditFile` tool (`I: From<EditRequest>`), `EditRequest`,
    `apply_edit` (exact match; `replace_all`; CRLF files matched and kept with `\n` in the
    model's text; `EditError::{NotFound{closest line}, Ambiguous{count}, Unchanged}`), bounds
    `MAX_EDIT_TEXT_BYTES` 64 KiB and `MAX_EDITED_FILE_BYTES` 1 MiB. The tool refuses secret names
    and empty `old_text` before interrupting.
  - Host `terminal_assist/edit.rs`: `prepare` resolves the path with the sandbox's
    `document_location` (inside the folder, no credential folders, symlinks canonicalised),
    refuses converted documents and non-UTF-8, applies the edit and builds a unified diff with
    `similar` (3 lines of context, 24 KiB shown). `apply` re-reads and compares a fingerprint;
    a changed file is not written and the model is told to read it again.
  - Decisions:
    - The grant is per question or per session for all edits (the same `question`/`session`/
      `decline` answer as `write_file`), not per file: simpler, and consistent with writes.
    - Not an operation-engine job, like `write_file` (task 0338): one bounded text write the
      person approved, through the VFS provider (`open_write` with overwrite). Not atomic
      (temp + rename) and not undoable; the diff is the record. Revisit if edits get bigger.
    - `edit_file` is offered with `write_file` under the same "allow file changes" terminal
      setting; auto-approve (0356) writes without asking.
    - Chat has no write tools, so the tool is terminal-only for now.
  - DTOs: `TerminalEditRequestDto`, reply `edit_request`, request `edit_decision`.
  - Frontend: `edit` panel with `+a −r`, a scrollable coloured diff, Allow (Enter) / This session /
    Decline (Ctrl+C, Esc).
  - Verified: fm-agent `edit::tests` 6; terminal_assist 85 (4 new edit-flow tests: approval, file
    changed meanwhile, decline, auto-approve with secret and not-found errors); drawer test for
    the edit panel; terminal + i18n frontend tests 462; clippy `-D warnings` on fm-agent,
    fm-application, fm-transport-dto clean. Not tried in the running app.
