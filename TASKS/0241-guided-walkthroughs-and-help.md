# 0241 Guided walkthroughs and in-app help

Status: blocked
Priority: medium
Subsystem: frontend
Depends on: 0025, 0030, 0068, 0070, 0079, 0104, 0238

## Context
Users need a way to discover Procyon's capabilities without a blocking feature tour. A
replayable basics demonstration should show real application UI, while a small library of
written task guides provides depth on demand. This task records the decisions made with the
user on 2026-10-09; it is not an interactive filesystem exercise.

## Acceptance Criteria
- Add `docs/walkthroughs/` with an index and task guides for basics, finding files,
  connecting over SSH/SFTP, remote transfer, semantic search, the collection basket,
  and organizing with workspaces. Link the SSH/SFTP setup guide from remote transfer.
- The basics guide and a short, captioned, replayable recording of real Procyon on macOS
  demonstrate two panes: the public Procyon repository on the left, `~/Downloads/test` on
  the right, with Downloads and Documents also visible as tab headers but without exposing
  their contents. Show F3 viewing `README.md`, F4 opening it for editing without modifying
  it, compressing it to a ZIP, and copying that ZIP to the right pane. Record in a staged,
  sanitized environment; playback must never create, change, or remove viewer files.
- Bundle the compressed clip for offline use and load it only when opened. Provide captions,
  playback controls, a poster or readable fallback, and reduced-motion-safe behavior.
  Label the clip as a macOS example; written steps must be platform-neutral.
- Make guides available through an in-app Help entry in both desktop and browser hosts,
  using `docs/walkthroughs/` as the source of truth and allowing offline reading. Keep
  controls localized; English guide content/captions are acceptable initially and should
  be identifiable as such. Preserve existing shortcut Help and diagnostics.
- On fresh installs only, show a one-time dismissible, non-modal invitation after a
  usable workspace opens. Never auto-play the clip; keep it replayable from Help.
  Existing installations must not receive an upgrade-time invitation.
- The semantic guide covers optional component setup, folder consent, indexing, search,
  and separately explains optional LLM configuration and ask/answer. The workspace
  concept gets a brief mention in basics and a dedicated guide.
- Cover first-run persistence, guide access and replay, host parity, and failure states
  with focused tests. Update the public README with a link to the guide index.

## Implementation Notes
- Follow `FileManagerClient` host parity and the existing Mithril/Meiosis state pattern;
  do not add a second client-side filesystem implementation. The Help interface may
  render bundled, build-time documentation, but should not fetch live GitHub content.
- Do not rely on the repository checkout existing on an installed machine; it is only
  a controlled recording example. Do not record private paths, credentials, real
  Documents/Downloads contents, or live Home Assistant connection details.
- SSH/SFTP examples must use placeholders and explain host-key verification; an SSH
  terminal is an optional capability, not a prerequisite for file browsing. Semantic
  search does not require an LLM profile.

## Agent Notes
- 2026-10-09: Worktree fast-forwarded to `origin/main` at `96d697a` before starting.
  Initial proposal to run a live tutorial was rejected as overkill; the agreed
  walkthrough is non-interactive media plus task-based written help.
- 2026-10-09: Wrote the seven offline guides under `docs/walkthroughs/`, linked them
  from README, and added a shared browser/desktop Help panel backed by bundled
  Markdown. The real, sanitized macOS screen recording, caption track, selective
  screenshots, and fresh-install invitation are **not delivered yet**. The app
  does not render a broken video player or offer a "Watch" invitation until
  the recording is available. Recording via the development mock would not
  truthfully demonstrate the requested Procyon checkout, `~/Downloads/test`,
  F3/F4, ZIP creation, and actual copy; do not substitute a mock slideshow.
  To unblock: capture the real desktop workflow in a controlled, sanitized
  session with public source data and no personal tab contents, then bundle,
  caption, and verify the clip before enabling the first-run invitation.
- 2026-10-09: The shared Help panel is exercised in mock and HTTP app-shell
  tests, the native Help action has spec/dispatch coverage, and the guide
  panel tests verify bundled content, cross-guide links, Escape, and scroll
  reset. Full frontend Vitest passed (153 files / 2363 tests before the final
  two focused tests), TypeScript/Vite build passed, and changed frontend
  files passed Biome. Workspace lint was started but stopped during the
  cold full-workspace Clippy build; no Rust sources changed.
