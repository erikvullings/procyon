# 0342 Skills and instruction files through the location's provider

Status: open
Priority: medium
Subsystem: backend
Depends on: 0341

## Context
Skills are discovered and read with `std::fs` only, so a working folder on an SSH (SFTP)
connection never shows the remote project's or the remote home's skills. The user expects remote
folders to be scanned too. The user also asked that the chat (and so the terminal assistant) can
read the project instructions an agent normally follows: `AGENTS.md` and `CLAUDE.md`.

## Acceptance Criteria
- `fm-agent` reads skills through a `SkillStore` trait (list a folder, file size and modification
  time, read bounded bytes, resolve the real path), with a local `std::fs` store as the default.
- A catalogue may hold skills from several stores; `read_skill`/`read_skill_file` read through the
  skill's own store, and the sandbox (no escape through symlinks or `..`, credential and secret
  names refused) holds for every store.
- `fm-application` backs the store with the working folder's VFS provider. For a remote working
  folder on a desktop host, the remote project's `.claude/skills` and `.agents/skills` (up to the
  remote git root) and the remote home's skill folders are scanned, next to the local home's.
  A browser client of a server still sees only the working folder's own skill folders.
- The header cache works for every store (keyed by store, path, size and modification time).
- Instruction files: `AGENTS.md`, `CLAUDE.md` and `.claude/CLAUDE.md` from the project (working
  folder up to the git root, outermost first) and, on a desktop host, `~/.claude/CLAUDE.md` and
  `~/.agents/AGENTS.md`, are added to the system prompt of both agents as a bounded "Project
  instructions" section, read through the same store (so remote projects work). The size bound is
  stated, and a cut file says how to read the rest (`read_file`).
- Tests with a fake store (remote-style paths, symlink escape refused, cache) and for the
  instruction section; `pnpm run lint` and the Rust suite pass.

## Implementation Notes
- `crates/fm-agent/src/skills.rs`, `skill_tools.rs`, new `store.rs` and `instructions.rs`.
- `crates/fm-application/src/agent_host/` provider-backed store, `skills_for`.

## Agent Notes
