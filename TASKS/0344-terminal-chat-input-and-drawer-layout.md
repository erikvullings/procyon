# 0344 Terminal assistant: chat input and drawer layout

Status: done
Priority: high
Subsystem: frontend
Depends on: 0343

## Context
User report: a `#` question was sometimes run by zsh (`zsh: command not found: #`), and icons
covered the typed text in the terminal. Two causes:
- After arrow keys or shell completion the typed line is "unknown", and the screen-line fallback
  fails when zsh wraps a long line without marking the row as wrapped.
- Block actions used `anchor: 'right', x: 0`. xterm.js only positions a right anchor for a non-zero
  `x`, so the icons sat at the left, over the prompt. Help and close floated over the first row.

## Acceptance Criteria
- `#` at an empty prompt opens a chat input under the terminal. It also opens before the shell is
  ready, from a paste, or when the screen shows an empty prompt. The shell never sees the question.
- The input stays open for follow-ups, with an accent edge and a hint line.
  - Enter sends and Shift+Enter adds a line.
  - Tab completes `@` and `/`; Up and Down recall earlier questions.
  - Esc, or Backspace on an empty input, goes back to the shell.
  - On an empty input, Enter, Esc, Ctrl+C and Ctrl+E answer a shown proposal, as they do in the shell.
- A question sent while an answer is coming is queued. It is sent after that answer; Esc puts it
  back into the input.
- A header row above the output holds the folder, Ask (`#`), help and close.
- A command block is a status dot at the right edge, and its actions show on hover or focus.

## Agent Notes
- 2026-11-23 claude: Added the pure module `question-composer.ts` (opening, empty prompt, caret
  helpers) with tests.
  - `terminal-drawer.ts` gained `composer`, `composerFocus` and `queued`, plus `openComposerFor`,
    `submitComposer`, `composerKey`, `completeInComposer`, `headerView` and `composerView`.
  - Removed the shell-side Tab and Up/Down `#` handling: the `onCompleteMention` and
    `onRecallQuestion` handlers are gone.
  - The Enter fallback `questionAtPrompt` is kept.
  - `openComposer` redraws synchronously, so keys typed right after `#` reach the input.
  - Added 7 strings and new `helpAsk`/`helpFollowUp` text in 8 locales.
  - Verified: the terminal tests and the i18n tests pass (370 in all), and tsc and Biome are clean.
  - Checked in a throwaway browser page with a fake shell (since removed):
    - `#` opens the input;
    - fast typing lands in it;
    - `@` completes;
    - the answer and the proposal show;
    - the dot expands on hover.
  - Not checked in the Tauri app, with real zsh or over SSH.
- 2026-11-23 claude: Follow-up: git status in the assistant's context.
  - `fm-vcs-status::working_tree_summary` (`summary.rs`, git2) returns:
    - the branch;
    - commits ahead of and behind the upstream;
    - up to 50 changed paths with `git status --short` codes.
  - Untracked folders are collapsed to one entry and ignored paths are skipped, so `target/` is
    never walked.
  - `terminal_assist::context` adds it as a separate `git` section of the state, so a session sends
    it again only when it changes.
  - The System pill leaves it out, and its tooltip now names it in 8 locales.
  - Tests: 2 in `fm-vcs-status`, 1 in context; clippy, fmt, tsc and Biome are clean.
  - Only for local folders: an SSH working folder gets no git status, as with the branch.
  - The status walk runs synchronously while the question is prepared.
- 2026-11-23 claude: Layout feedback and agent fixes.
  - Removed the header row. The last line of the strip is now a status line:
    - in the shell it shows the folder, Explain/Fix when there are command blocks, then Ask, help
      and close;
    - in the chat it shows the context pills, the actions (thinking, Explain/Fix, Save as note),
      then Ask, help and close.
  - The chat input sits above it. Its hint text gave way to a send arrow (`cornerDownLeftIcon`) at
    the bottom right; the keys moved to a new help entry, `helpKeys`. The removed "×" dismissed
    the bar.
  - fm-agent: after a call tag, an object that follows a stray word now counts as a call. Gemma
    wrote `<|tool_call>_call {"tool":...} text`, which was dropped, so the turn ended without
    acting.
  - llm_profiles: a refused generation (any non-2xx) now logs its status and up to 2 KiB of the
    redacted body at warn level. "Assistant unavailable" was undiagnosable before: the body was
    discarded.
  - Verified:
    - terminal tests 243 and i18n tests pass;
    - fm-agent 90 and llm_profiles 67 Rust tests pass;
    - clippy, fmt, tsc and Biome are clean;
    - checked both status-line modes visually in a throwaway page.
- 2026-11-23 claude: Second round of layout and agent fixes.
  - Close went back to the top right. It sits in a 22px right gutter of `.fm-terminal-host`, so it
    covers no output.
  - Removed the 6px bottom margin under the pills.
  - `questionAnsi(question, cols)` wraps at words with a 2-space hanging indent, leaving one column
    free.
  - Timeouts:
    - A model call waits at least 5 minutes for its first chunk, not just for the headers. Servers
      that send headers at once and then read a long prompt timed out on the short idle limit.
    - The idle timer still restarts with every chunk, and each call gets a fresh wait.
  - New `ChatFailureDto::Timeout`, with messages in the chat and the terminal (8 locales). The
    OpenAPI document and client were regenerated.
  - `chat_failure` now logs every failed model call except a cancelled one, at warn level.
