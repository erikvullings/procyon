# 0342 Skills and instruction files through the location's provider

Status: done
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
- Instruction files: `AGENTS.md`, `CLAUDE.md` and `.claude/CLAUDE.md` from the project (a local
  working folder up to the git root, outermost first; a remote one only itself) and, on a desktop
  host, `~/.claude/CLAUDE.md` and `~/.agents/AGENTS.md`, are added to the system prompt of both agents as a bounded "Project
  instructions" section, read through the same store (so remote projects work). The size bound is
  stated, and a cut file says how to read the rest (`read_file`).
- Tests with a fake store (remote-style paths, symlink escape refused, cache) and for the
  instruction section; `pnpm run lint` and the Rust suite pass.

## Implementation Notes
- `crates/fm-agent/src/skills.rs`, `skill_tools.rs`, new `store.rs` and `instructions.rs`.
- `crates/fm-application/src/agent_host/` provider-backed store, `skills_for`.

## Agent Notes
- 2026-11-23 Claude: Done.
  - `fm-agent/src/store.rs`: `SkillStore` (`id`, `list`, `metadata`, `read`, `real_path`),
    `StoreHandle` (compared by id), `LocalStore` (`std::fs` on the blocking threads; Windows
    `\\?\` stripped), `join_path`/`relative_to`/`file_name` with `/` and `\` both separators.
  - `skills.rs` is async and store-based: `SkillRoots` holds store paths (`standard` for the local
    disk, `working_folder(folder)` for any store), `SkillCatalogue::discover(store, roots)` and
    `merge`, `SkillLocation::Folder { store, path }`, `SkillIndex` keyed by (store id, path).
    `resolve_file`/`read_checked`/`list_files` run through the skill's own store, with the same
    sandbox rules. `SkillDocuments::read(store, path, offset)`.
  - `fm-agent/src/instructions.rs`: `instruction_candidates`, `read_instructions` (dedupe by real
    path, binary skipped, 16 KiB in all, the crossing file cut), `instructions_section` (cut files
    say `read_file <path>` when the working folder holds them).
  - `fm-application`: `agent_host::project_context` replaces `skills_for` and returns skills and
    the instructions section. Local folder: project skills up to the git root, instructions from
    the git root down (only the working folder without a git root), `~/.claude/CLAUDE.md` and
    `~/.agents/AGENTS.md`, the person's skill folders. Remote folder (no `file:` path):
    `agent_host/skill_store.rs` `ProviderStore`, paths relative to the working folder, links and
    `..` refused; only its own `.claude/skills`, `.agents/skills` and instruction files, plus the
    local home's skills and instructions on a desktop host. Server browser clients
    (`SkillScope::WorkingFolder`): the working folder's own only. Skill documents on a remote
    store are read as plain text (the converter only takes local paths).
  - Both prompts carry the section (chat after `TOOLS_INTRO`, terminal after the ask intro).
  - Verified: `fm-agent` 89 unit tests + doctest (new: store, instructions, remote-style store);
    `fm-application` 1546 tests (new: provider store keeps to its folder and refuses links,
    instructions order and git-root rule, chat prompt carries AGENTS.md/CLAUDE.md); clippy clean.
  - Not verified over a real SSH connection; `ProviderStore` uses the same provider calls as the
    remote file tools (`inspect`, `list`, `read_range` with an `open_read` fallback).
