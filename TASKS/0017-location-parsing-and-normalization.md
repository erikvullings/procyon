# 0017 Location parsing and path normalization

Status: done
Priority: high
Owner: unassigned
Agent: unassigned
Area: backend
Depends on: 0016

## Context
`file-manager-coding-agent-spec.md` §5.1 and §33 step 4. Locations are provider-neutral URIs, not
`PathBuf`s. Correct parsing is security-critical (§22: never accept arbitrary paths without
validation) and is exercised by every later feature.

## Acceptance Criteria
- Bidirectional conversion between `Location` and a native path for the `local` provider, covering:
  - POSIX (`file:///Users/erik/Documents`),
  - Windows drive paths (`file:///C:/Users/Erik/Documents`),
  - Windows UNC paths (`file://server/share/dir`),
  - percent-encoding of spaces and shell-sensitive characters,
  - Unicode names (including non-NFC forms on macOS),
  - very long paths (Windows `\\?\` prefixing where needed).
- Normalization resolves `.`/`..` lexically without touching the filesystem and rejects escapes
  above a configured root.
- `parent()`, `join(name)` and `name()` helpers that never use unsafe string concatenation (§5.1).
- Round-trip property tests (`proptest` or table-driven) prove `path → Location → path` is lossless
  for the cases above.
- Rejects with a typed error: null bytes, empty segments, mismatched provider scheme, and reserved
  Windows device names (`CON`, `NUL`, `COM1`, ...).
- Tests run on macOS, Windows and Linux in CI; platform-specific cases are `cfg`-gated, not skipped
  silently.

## Implementation Notes
- Keep the URI syntax stable enough for bookmarks and history (§5.1); document it in
  `docs/architecture/locations.md`.
- Reserve the `archive://`, `search://` and `sftp://` schemes in the parser's provider dispatch, but
  return "unsupported provider" for them.

## Agent Notes
- 2026-07-30 codex: Implemented validated provider-neutral location parsing in `fm-domain`.
  `file:` URIs dispatch to provider ID `local`; `archive`, `search` and `sftp` are reserved with a
  typed unsupported-provider error. Added typed failures for malformed URIs and percent encoding,
  null bytes, empty segments, provider/scheme mismatches, unsafe child names, root escapes and
  reserved Windows device names.
- 2026-07-30 codex: Added native path conversion for POSIX, Windows drive and UNC paths, including
  byte-preserving Unix encoding for decomposed macOS names and automatic Windows `\\?\`/`\\?\UNC\`
  prefixing at the legacy path limit. `normalize_within`, `parent`, `join` and `name` operate
  lexically on decoded path segments without filesystem access or URI string concatenation.
- 2026-07-30 codex: Followed TDD through the public `Location` API. Six task-specific contract
  tests cover provider dispatch and typed rejection, root-constrained normalization, helpers,
  Unicode/shell-sensitive POSIX paths, platform-gated Windows drive/UNC/long paths, and a proptest
  native-path round trip. Verified independently with `cargo test -p fm-domain --test
  location_contract` (6 tests), plus `cargo test -p fm-domain`, `cargo check -p fm-domain
  --all-targets`, and Clippy with `-D warnings`.
- 2026-07-30 codex: Verified `pnpm test` passes the full Rust workspace, 70 frontend tests and 28
  script tests. Repository-wide Rust formatting and Clippy pass. `pnpm run lint` remains non-zero
  only for pre-existing, task-unrelated Biome findings in `frontend/vite.config.ts`,
  `scripts/architecture-docs.test.mjs` and `scripts/ci-workflow.test.mjs`; none was changed.
  `CLAUDE.md` is absent, so there was no scoped file to update. Windows-specific tests are
  `cfg(windows)` and run in the existing CI OS matrix, but were not executable on this macOS host.
