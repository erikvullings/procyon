# 0333 Local LLM assistance in the terminal pane

Status: in_progress
Priority: medium
Subsystem: frontend, backend, settings
Depends on: 0126, 0322

## Context
The embedded terminal drawer (0126, xterm.js + `portable-pty`, Tauri only) and the chat tab (0324) both exist. The user wants the existing LLM setup (endpoint profiles and roles, 0322) to power terminal assistance, comparable to Warp's AI terminal features, but independent of any cloud service.

Warp's AI terminal features (from Warp docs and blog; cloud-backed by default, data sent to OpenAI/Anthropic):
- Natural language to command, with the command placed in the input for review before running.
- Blocks: each command grouped with its output, so one block can be asked about or explained.
- Explain an error or fix a failed command from the block output.
- Next-command and history-based suggestions.
- Agent mode: multi-step tasks, each command shown for approval before running.

Procyon's rule for this feature: AI only uses the configured LLM endpoint and roles. Cloud endpoints need the existing allow-cloud consent (0322). No command runs without an explicit user action.

## Acceptance Criteria
Phase 1 (confirmed by the user):
- Natural language to command: a key or `#` prefix in the terminal opens a prompt. The LLM returns one shell command, shown in an editable preview. Enter inserts it into the prompt; it is never executed automatically.
- Context sent to the LLM: current working directory, OS, shell name, and optionally the last N lines of terminal output. The user sees exactly what is sent.
- Explain last output: a control on a finished command explains its output or error in the chat tab, reusing the chat context path (0324).
- Suggest fix after a non-zero exit: shows a suggested corrected command, same review-before-run rule.

Phase 2 (confirmed by the user; depends on shell integration, below):
- Command blocks: each command and its output grouped; copy, collapse, rerun.
- Next-command suggestions from shell history, local only, shown inline and accepted with a key.

Out of scope for this task: Warp-style agent mode (multi-step, per-command confirmation). Track separately if wanted later.

Cross-cutting:
- Works with any configured local endpoint (e.g. Ollama, LM Studio) and never sends terminal output to a cloud endpoint without the 0322 consent.
- Feature can be turned off in settings.
- Browser mode: the terminal is Tauri-only today; state explicitly whether the AI features follow that or are hidden in browser mode (CLAUDE.md parity rule).

## Implementation Notes
- Shell integration: OSC 133 prompt/command markers, emitted by an opt-in shell snippet (zsh/bash/fish first). This is needed for blocks, exit codes, and "last command output". Without it, fall back to scrollback text only.
- Reuse the LLM role resolution from 0322 and chat context plumbing from 0324; do not add a second LLM path.
- Keep the terminal output scan and LLM call in the backend capability service; frontend renders the preview only.
- Prompts and responses must not be persisted outside the existing chat conversation store.

## Agent Notes
- Created from user request after a feature survey of Warp. User confirmed phases 1–2; agent mode dropped from scope.
- Dead end noted: Warp's own pages don't document their terminal AI features in detail; the summary is from search snippets and secondary sources. Re-check Warp's docs before final scoping if exact behavior matters.
- 2026-11-13 Claude: Phase 1 implemented (uncommitted). Phase 2 (OSC 133 blocks, history
  suggestions) not started.
  - Backend: `fm-transport-dto/src/terminal_assist.rs` (kind `command|explain|fix`, transcript,
    request, working directory, shell; reply `text`). `fm-application/src/terminal_assist.rs`
    answers through `LlmRole::Chat` via the existing `ChatGenerator`, saves nothing, caps input
    (16 KB, newest kept) and output. Failures reuse `ChatFailureDto`. Tauri command
    `terminal_assist` in `apps/fm-desktop/src-tauri/src/commands.rs`, registered in `lib.rs`.
    7 unit tests pass; clippy `-D warnings` clean for the touched crates.
  - Frontend: `terminal/terminal-assist.ts` (pure helpers: transcript tail, command extraction,
    clean-up; 11 tests), `terminal-client.ts` `assist()`, `terminal-drawer.ts` overlay with
    Ask / Explain / Fix buttons, an editable answer panel, and Insert at prompt. Insert types the
    command into the PTY without Enter. Overlay CSS in `themes/theme.css`. Strings in all 8
    locales. Drawer tests cover no-output and ask-then-insert.
  - Deviations from the agreed phase 1 wording, for the user to confirm:
    - Triggers are buttons, not a `#` prefix or shortcut. No keyboard binding yet.
    - Explain and Fix answer in the terminal overlay, not the chat tab. Nothing is saved.
    - Fix detects failure from the output text, not an exit code. The exit code needs phase 2's
      OSC 133 markers.
    - Shell name is not sent yet (`unknown`). Requests are not cancellable.
  - Not verified in the app: the desktop build runs only under Tauri, and the browser pane has no
    terminal. Checked by unit tests and typecheck, not by running the drawer.
  - Dead end: none recorded. CSS must use the `--fm-radius` token; `theme.test.ts` rejects raw
    `border-radius` values in feature styles.
- 2026-11-13 Claude: Reworked phase 1 per user: `#` at the prompt asks (no buttons); Explain and
  Fix are block icons; the assistant stays separate from the chat pane (no conversation, no
  storage, shared chat endpoint only). Phase 2 implemented (uncommitted).
  - Shell marks: `frontend/src/features/terminal/shell-integration.ts` (OSC 133 A/C/D snippets for
    zsh, bash, fish; `parseShellMark`). zsh and bash verified in a real PTY (ordering A/C/D right).
    fish NOT run (not installed here): `$status` in `fish_postexec` unverified.
    Dead end: zsh's `status` is read-only; the snippet uses `exit_code`.
  - Blocks: `block-tracker.ts` (pure; prompt → output → end; empty Enter makes no block; keeps 200).
    `terminal-drawer.ts` puts an xterm decoration on each finished block's prompt line with icons:
    Explain, Suggest fix (failed blocks only), Copy output, Run again (writes the command + Enter on
    an explicit click). Collapse is not done.
  - `#` questions: `typed-input.ts` tracks the typed line. A submitted line that starts with `#`
    is not run: Ctrl+U clears the shell line and the question goes to the assistant. Only at a
    prompt (or before any shell mark is seen).
  - Suggestions: from commands typed in this terminal session only, not the shell history file.
    Right arrow accepts; shown as a chip in the overlay.
  - Setup: until the shell reports marks, the overlay shows the snippets and how to ask (`#`).
  - Tests: block-tracker, typed-input, shell-integration, drawer `#` flow (paste + Enter), helper
    tests. Decorations and icons are not covered by tests (xterm rendering in jsdom).
  - Not verified in the app: decoration rendering and icon placement. Needs a Tauri run.
  - Unrelated failures in the working tree: `frontend/src/features/image-generation/*` (someone's
    uncommitted work) breaks tsc and 21 tests. Not touched.
- 2026-11-13 Claude: Shell integration is now automatic, with one-time consent (replaces the manual
  snippets). Not committed yet at the time of writing; see git log for the commit.
  - Setting `terminalShellIntegration` (`unset` | `enabled` | `disabled`) in fm-settings, DTO,
    mapping, and the generated client. Default `unset`: the drawer asks once. "Enable" restarts the
    current tab's shell; the other tabs pick it up when they next start.
  - Rust `apps/fm-desktop/src-tauri/src/shell_integration.rs`: wrapper files in the app data folder
    (`shell-integration/`). zsh: `ZDOTDIR` points at the wrappers, which source the person's own
    `.zshenv`/`.zprofile`/`.zshrc` and then install the hooks. bash: `--init-file` wrapper that runs
    the login startup (`/etc/profile`, first of `.bash_profile`/`.bash_login`/`.profile`), then
    `.bashrc` unless the profile already reads it, then the hooks. Dotfiles are not changed.
  - Sessions carry a generation id, so a closed session's exit cannot remove a restarted one.
    `close_embedded_terminal` ends a session; events and ids from a superseded open are ignored.
  - Verified: wrapper unit tests, and real zsh and bash PTY tests (the person's rc loads, `C` and
    `D;1` marks appear), clippy `-D warnings`, frontend suite (except the in-progress image pane),
    typecheck.
  - Not done: fish (its `$status` in `fish_postexec` is unverified; not installed here); remote SSH
    shells; a Settings toggle to change the choice later (only the consent prompt sets it).
