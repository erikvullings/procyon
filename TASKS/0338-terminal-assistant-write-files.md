# 0338 Terminal assistant: create files (summaries, reports) with a batch approval

Status: done (unverified in the Tauri app and with a live LLM)
Priority: medium
Subsystem: backend, frontend, settings
Depends on: 0336, 0337

## Context
The terminal assistant (0336) reads (`list_dir`, `read_file` incl. PDF/Word/EPUB, `grep`), proposes
notes for review (0337) and runs single-line commands after approval. It cannot write files. A
request such as "summarise every document in this folder into a Markdown file each" is therefore
impractical: 30 notes to review one by one, or 30 one-line shell commands (newlines are rejected,
auto-run refuses `>`).

Decision (with the user, 2026-11-21): add a **create-only** write tool with a **batch approval**.
Editing existing files is out of scope here (later, only through an approved diff).

## Threat model
Everything the model reads is untrusted (a PDF can say "write this to `~/.zshrc`"). The tool must
stay safe even when the model is fully steered by a document. Therefore the tool cannot choose
*where* freely, cannot replace anything, and cannot write executable or credential-like files.

| Risk | Control |
| --- | --- |
| Overwriting a real file | Create-only (`WriteOptions { overwrite: false }`, atomic create-new). A name clash gets a numbered suffix (`name-2.md`); the result tells the model the final name. |
| Writing outside the folder / symlink escape | Same sandbox as `read_file` (`Sandbox::resolve`): relative path only, no `..`, no absolute path, no symlink components, no credential names. |
| Startup / config / executable files | Extension allow-list `.md` and `.txt`; refuse names starting with `.`; refuse any path segment that is a hidden folder. |
| Wrong place by injection | Every file goes under one **output folder** chosen at approval time (default `<cwd>/summaries`, or the notes folder). The tool takes a file name relative to it; the first call of a batch fixes the folder. |
| Secrets copied into a summary | `redact_secrets` on the content before writing; the result reports how many values were replaced. |
| Runaway volume | Per-file cap (64 KiB), per-question cap (50 files, 1 MiB total); beyond that the tool returns an error and the model must ask. |
| Silent action | Nothing is written before the batch is approved; every written file is listed in the terminal (path, size). |
| Remote hosts | Same VFS provider path as reads (local, SFTP); no special case. |

## Design

### Tool
`{"tool":"write_file","path":"report.md","content":"..."}` (text protocol like the other tools;
`content` may be multi-line JSON string). Validation lives in `protocol.rs` next to `ReadFile`
(name, extension, size); the sandbox rules in `tools.rs`.

### Approval (batch, not per file)
1. The first `write_file` of a question does not write. The loop pauses and the reply carries a
   `pendingWrite` request: output folder, the intended file name, size. The assistant continues
   once the person answers.
2. The strip shows **"Write files to `./summaries/`"** with: **Allow for this question**, **Allow
   for this session** (this tab's session only, same output folder), **Decline**. Keyboard hints
   like the approve row. Auto-run does not apply (this is never "read-only").
3. After approval the model's later `write_file` calls in that question (or session) are written
   without asking, still bounded by the caps and the output folder. A write outside the approved
   folder asks again.
4. The result of each call goes back to the model (`Created summaries/a.md (1.2 KiB)`), and the
   terminal prints one dim line per file, like the "Read file" steps.
5. Declined: tool result `declined`, nothing written; the model is told not to retry.

### Implementation sketch
- `crates/fm-application/src/terminal_assist/write.rs` (new): `FileWriter` with the caps,
  suffixing, redaction and the VFS `open_write` call; unit-tested with a temp root (traversal,
  symlink, extension, overwrite, caps, remote provider via the in-memory fake).
- `protocol.rs`: `ToolCall::WriteFile { path, content }`; `mod.rs`: loop pauses like `run_command`
  (the pending state reuses the session's pending-approval machinery) and carries the approved
  folder and counters in the session.
- Transport DTO + frontend `TerminalAssistReply.pendingWrite`; strip view for the approval; the
  streamed steps show created files. Browser host: not applicable (terminal is Tauri-only).
- Setting (Terminal): "Let the assistant create files (after approval)", default **on** because
  every write is approved; off removes the tool from the prompt.

### Prompt
Describe the tool, state that it only creates new `.md`/`.txt` files in the approved folder, that
file names must be short and relative, and that existing files are never changed. Tell the model to
write one file per document and to keep summaries factual.

## Acceptance Criteria
- "Summarise each document in this folder into its own Markdown file" asks **once**, then creates
  one file per document in `./summaries/`; each is listed in the terminal; nothing is written
  before approval.
- An existing file is never overwritten (suffix instead); the model sees the final name.
- `..`, absolute paths, symlinks, hidden names, other extensions and credential names are refused
  with a clear tool error (tests for each).
- Content with a secret has it replaced before writing; the result says how many.
- Caps are enforced (file size, count, total) with tests.
- A prompt-injected "write to ~/.zshrc" cannot be expressed: refused by the sandbox and the
  allow-list (test).
- Declining writes nothing and the model does not keep retrying.
- Works for local and SFTP folders. Setting off removes the tool from the prompt.
- `pnpm run lint` clean; Rust and frontend tests added; behaviour in the Tauri app and with a live
  LLM reported as verified or not.

## Decisions
- Output folder is fixed to `./summaries/` under the terminal's folder (user, 2026-11-22). The model
  passes only a plain file name.
- Notes: not covered by this tool. Real notes keep `propose_note` (reviewed one by one, indexed).
  A batch of notes is created in `summaries/` and moved/imported by the person; a later task may add
  a `destination: notes` that writes through the notes service after the same batch approval.
- "Allow for this session" is keyed by working folder + output folder, so a folder change asks
  again.

## Agent Notes
Built 2026-11-22: `terminal_assist/write.rs` (`FileWriter` over the VFS provider, so local and SFTP
are the same; create-new with `-2`, `-3` suffix; symlinked/non-directory output folder refused;
name allow-list; redaction count), `ToolCall::WriteFile`, session pending write + grant + per-question
counters (50 files, 1 MiB), DTO `writeDecision`/`allowFileWrites`/`writeRequest`, setting
`terminalAllowFileWrites` (default on; off removes the tool from the prompt), strip "Create files in
./summaries/?" with Allow (Enter) / This session / Decline (Ctrl+C or Esc). Tests: Rust writer and
loop flow (approve, per-question vs session grant, decline, bad name, setting off, count limit),
frontend drawer flow. Not done: SFTP test against a real server, Tauri/LLM run.
