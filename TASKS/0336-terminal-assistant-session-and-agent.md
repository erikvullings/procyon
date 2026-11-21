# 0336 Terminal assistant: session, full context and agent loop

Status: in_progress
Priority: medium
Subsystem: backend, frontend, settings
Depends on: 0126, 0322, 0324, 0333

## Context
0333 shipped one-shot terminal assistance (`#` ask, Explain/Fix block icons, suggestions) through
`crates/fm-application/src/terminal_assist.rs`. In use it falls short:
- No session: every call is a system prompt plus one user prompt. No history, no memory of the task.
- Output blind: `Command` (`#`) never sends the transcript; only `Explain`/`Fix` do.
- Explain/Fix only exist as block icons, which need OSC 133 shell integration (consent `unset`
  by default). Fix guesses failure from text, not the exit code.
- Thin context: `Shell: unknown` plus a cwd URI. No OS/arch, real path, folder listing, git state,
  or pane selection; no way to attach a file.

Goal: behave like Warp's agent. Session-aware, sees the terminal, and can run a bounded
multi-step loop with per-command approval. This reopens the "agent mode" scope 0333 excluded;
confirm with the user before starting phase 3.

Rules kept from 0333: only the configured LLM endpoint and roles (0322); cloud endpoints need
the allow-cloud consent; no command runs without an explicit user action; feature can be turned
off; Tauri-only like the terminal (hidden in browser mode).

## Design
A small pi-style harness (few tools, short prompt, no framework) in `fm-application`:
`TerminalAgentService`, next to `ChatGenerator`. No new crate. `FileManagerService` only delegates.

### Session
- One in-memory session per terminal tab: message list, bounded, summarised when long.
- `LlmChatGeneration` takes a message vector, not just system + user prompts.
- Not persisted. Optional "save to chat tab" reuses `chat_conversations`.

### Context (always attached; shown as removable chips; the user sees exactly what is sent)
- Environment: OS, arch, shell, real cwd path, git branch.
- Terminal: last N blocks (command, exit code, output), head+tail truncated, secrets redacted.
  Scrollback fallback without OSC 133.
- Folder: bounded names-only listing of the cwd.
- Procyon-specific: selected files in the active pane, the other pane's folder, `@file`, `@dir`,
  `@output` mentions.

### Unified input
- One prompt box; the model decides ask / explain / fix.
- Explain and Fix remain as chips prefilled with the last block and are always available, not
  gated on shell integration.

### Tools (phase 3+)
- Auto-approved, read-only, run in Rust over the VFS: `list_dir`, `read_file` (bounded), `grep`.
- `propose_command`: inserts into the prompt without running (today's behaviour).
- `run_command`: needs approval each time. Runs in the visible PTY; the OSC 133 D mark returns exit
  code and output to the loop.
- No write/edit tool: file changes go through the operations engine (spec rules 6 and 8).
- Add tool-calling to `LlmChatGeneration` (the Ollama probe already reads the `tools` capability).
  Models without tools: JSON-in-text fallback.
- Limits: max steps, cancellation, stop on rejection, per-call output cap.

### Host split
Loop runs in Rust and emits tool-request events over a Tauri channel. Frontend asks for approval,
runs the command in the PTY, posts the result back.

## Acceptance Criteria
Phase 1 — session and context:
- Follow-up questions in the same tab see earlier turns; closing the tab drops the session.
- Every request carries OS, arch, shell, real cwd, git branch, a bounded cwd listing and the last
  N blocks, including `#` asks.
- A context panel shows exactly what is sent; chips can remove items; one-click share of the folder
  listing or a chosen file's contents.
- Cloud endpoint: output/files never sent without 0322 consent.
Phase 2 — always-on Explain/Fix, unified input, `@` mentions, selection and other-pane context.
Phase 3 — tool-calling plumbing and the read-only tools; step limit and cancel work.
Phase 4 — `run_command` with approval, exit code and output returned to the loop; rejection stops it.

## Implementation Notes
- Extend `terminal_assist.rs` or replace it with the new service; keep `TerminalAssistKindDto`
  compatible until the frontend moves over. Regenerate the client via `pnpm api:generate`.
- Secret redaction needs tests (tokens, `export KEY=...`, `Authorization:` headers).
- Tests: session turn accumulation, truncation, context assembly, tool loop with a fake
  `ChatGenerator`, approval/reject paths, cloud-consent refusal.
- Browser parity: state that the feature follows the terminal (Tauri-only).

## Agent Notes
- Created 2026-11-20 from the user's review of 0333's assistant.
- 2026-11-20 Claude: all four phases implemented (uncommitted at time of writing; see git log).
  - Backend `crates/fm-application/src/terminal_assist/`: `session.rs` (per-tab memory, 32 sessions
    LRU, 24 KB prompt bound), `context.rs` (OS/arch for local shells only, shell, real cwd, git
    branch read from `.git/HEAD`, other pane, selection, listing capped at 100, last blocks, `@path`
    files), `redact.rs` (key blocks, secret-named pairs and flags, token shapes), `protocol.rs`,
    `tools.rs` (list_dir, read_file, grep), and the `ask` loop in `mod.rs`. 44 unit tests.
  - Deviation: tool use is the JSON-in-text protocol (`TOOL {json}`) for every endpoint. Native
    tool-calling in `LlmChatGeneration` was NOT added; it is only needed for models that follow the
    text protocol badly. Session history is folded into the prompt text, as chat.rs does, so
    `LlmChatGeneration` did not change.
  - Deviation: no host-initiated loop. `ask` returns at a `run_command` request (`runCommand`);
    the frontend approves, runs it in the PTY, and posts `toolResult` back on the next request.
    No Tauri channel was needed. Requests are still not cancellable.
  - Read-only tools only look inside the terminal's working folder (canonicalised, symlinks out are
    refused), skip secret-looking names (`.env*`, `id_*`, `*.pem`, ...) and redact output. They are
    unavailable for SSH shells (the PTY command path still works).
  - `run_command` needs shell integration marks (exit code and output come from OSC 133); without
    marks the approval panel offers Insert instead of Run.
  - Frontend: `#` asks go through `ask`; context chips (toggle per part, `@dir`/`@output`/`@path`
    mentions) show while typing a `#` question and under every answer; follow-up field; Explain and
    Fix work without blocks (scrollback fallback) and without a failed exit status; approve/decline
    panel; `assistClose` on tab close. Tauri command `terminal_assist_close` added.
  - Bug found and fixed on the way: the xterm `Terminal` had no `allowProposedApi`, so
    `registerDecoration` (the block icons) threw inside the OSC 133 handler. 0333's icons could not
    have rendered in the app.
  - Not verified: the Tauri app, a live LLM (any endpoint), the fish/zsh/bash flows end to end,
    and the new chip/approval CSS visually. Unit tests only (backend fake generator, jsdom drawer).
  - Not done: native tool-calling, cancellation, `@`-completion UI, a git branch override, a
    Settings toggle for the assistant.
- 2026-11-20 Claude: layout rework after first use. The conversation is now written into the
  terminal itself (`answer-ansi.ts` turns the Markdown answer into styled, wrapped terminal text;
  control characters from the model are stripped, so it cannot send escape sequences). The question
  replaces the prompt line, the answer follows, and the shell prompt returns with an empty Enter.
  Keys typed meanwhile are held and sent after the prompt is back; a half-typed command is set
  aside with Ctrl+U and put back with Ctrl+Y (Explain/Fix from the bar or shortcut). Only a slim
  bar stays below: pills (hover tooltip via CSS `data-tooltip`; the webview shows no native
  `title`), the command to insert/approve, and Explain / Fix. No follow-up field: the next `#`
  question is the follow-up. Enter inserts or runs the offered command only while the prompt line
  is empty. `@dir` and `@output` were dropped (the pills already cover them); `@file` stays.
  Not verified in the Tauri app: the prompt return and held-key flow need a real zsh/bash/fish.
- 2026-11-20 Claude: fixes after the inline-conversation rework.
  - Tool calls: the parser now accepts the first JSON object anywhere in a reply that names a known
    tool (also inside `<tool_call>` tags, code fences, or as `{"name","arguments"}`); a model that
    answered with XML-wrapped JSON used to show it as text.
  - Context: the prompt shows the terminal's current state (system, folder, selection, listing)
    once at the top and replaces it when it changes; command blocks already sent are not sent
    again, and each question keeps the activity it came with. (The chat API is stateless, so the
    request still carries all of it; the saving is no repeats, not no resend.)
  - Folder listing: left out by default above 64 names until the pill is switched on; the backend
    lists up to 1000 names. The pane may hold only the first 256 of a huge folder; `list_dir` (200)
    can look further.
  - Typing before the shell is ready is held and replayed (it was echoed once by the tty and again
    by the shell), ready = first prompt mark, or output quiet for 250 ms (1.5 s when marks are
    expected), or 400 ms after open.
  - Tooltips are drawn by the drawer after 500 ms (CSS `:hover` and native `title` did not show);
    pill tooltips read "Included. Click to exclude." / "Not included. Click to include.".
- 2026-11-20 Claude: streaming, folding pills, settings, append-only context.
  - Context is append-only: the state (system, folder, other pane, selection, listing) goes in
    with the first question, afterwards only the parts that changed are added ("Terminal state
    changed:"), and nothing already sent is altered, so a cached prompt prefix still matches and an
    old command is never taken to have run in the new folder.
  - Streaming: `ask` reports each finished line and each step on a Tauri channel
    (`terminal_assist_stream`, `TerminalAssistEventDto`); the terminal writes them as they arrive
    (`createAnswerStream` renders line by line). Granularity is a line, not a token: a long
    paragraph still appears when its line is complete. Lines that look like a tool call are held
    back, and shown at the end only if the reply turns out not to be one.
  - The pills fold into one "Context (n)" button when the bar is under 640 px (CSS container
    query); the button opens them.
  - Setting `terminalAssistant` (default on; Settings → Terminal): off leaves `#` to the shell and
    hides the bar, help, block Explain/Fix and the shortcuts.
  - Not verified in the Tauri app: streaming over a real channel, the container query, the layout.

### Follow-up notes: folder sync, documents, completion, cancel

- **Folder sync** (`terminalFollowPane`, default on, Settings → Terminal): the local shell hooks print
  `OSC 133;P;Cwd=<path>` at each prompt; the drawer follows it with the active pane
  (`onFollowFolder`), and a pane folder change types `cd -- <path>` at an idle empty prompt
  (`paneFolderChanged`). `LiveTerminal.folder` is the last shared folder and prevents ping-pong.
  SSH shells have no hooks, so they are not synced.
- **Documents:** `read_file` and `@file` read EPUB/DOCX/PDF/PPTX/XLSX through
  `DocumentConversionService` (`terminal_assist/documents.rs`); `offset` continues into the
  converted text. Each continuation converts again (no cache yet).
- **`@` completion:** Tab lists the terminal folder (`listFolder`), quotes names with spaces.
- **No step cap:** the tool loop runs until answered; ESC calls `terminal_assist_cancel`, which ends
  it before the next model call. A model call already in flight still finishes.

- SSH folder sync: with shell integration on, the desktop host types `REMOTE_HOOK_LINE`
  (`shell_integration.rs`) into a remote bash/zsh once its first output has been quiet for 400 ms,
  MobaXterm style. It installs the same OSC 133 marks (so blocks and idle detection work over SSH
  too); the echoed line is drawn invisible (SGR 8) and reset by the line itself. Other remote
  shells (fish, csh, ...) are untouched and not synced.
- Fish remote hook added as a second guarded eval (line < 1000 bytes: canonical-mode cap).
- `Documents` caches converted text per question (4 documents), so `offset` continuations do not
  convert again; a new question starts with an empty cache.
