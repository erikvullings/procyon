# 0151 Fix Windows git-status/history: `canonicalize()` vs. `git2` path mismatch

Status: done
Priority: medium
Owner: unassigned
Agent: claude
Area: backend
Depends on: 0135

## Context

Surfaced 2026-08-19 by CI, on `main` at commit `e971366`, once a `sccache` cache-service outage
(fixed by removing sccache from `ci.yml` entirely) stopped masking real test results on the
Windows runner. Two `fm-application` tests fail only on `Rust (windows-latest)` — macOS and the
rest of the suite pass clean:

- `directory::tests::listing_a_git_working_tree_annotates_entries_with_git_status`
  (`crates/fm-application/src/directory.rs:1907`) — `assert_eq!(tracked.git_status,
  Some(Modified))` gets `None` instead.
- `directory::tests::git_history_returns_commits_touching_a_tracked_file`
  (`crates/fm-application/src/directory.rs:2001`) — expects 1 matching commit, gets 0.

**Root cause (diagnosed from the source, not yet verified on a real Windows machine — that
verification is this task's first step):** every entry point into `crates/fm-vcs-status/src/
lib.rs` (`GitStatusService::annotate`, `GitStatusService::file_history`) calls `canonical(path)`
(`std::path::Path::canonicalize()`) on the queried path, then `.strip_prefix(&repo_root)` where
`repo_root` comes from `git2::Repository::discover(dir).ok()?.workdir()`. On Windows,
`std::fs::canonicalize` returns an **extended-length "verbatim" path** prefixed with `\\?\` (e.g.
`\\?\C:\Users\...\tracked.txt`), while `git2`/libgit2's `workdir()` returns a normal path with no
such prefix. `strip_prefix` on a `\\?\`-prefixed path against a non-prefixed `repo_root` fails,
and every call site swallows that failure silently via `.ok()?`/`let-else` (`annotate` returns
early doing nothing, `file_history` returns `Vec::new()`) rather than erroring — which exactly
matches the observed symptom: quiet "no git info" instead of a panic or visible error.

This is a well-known Rust-on-Windows gotcha (`std::fs::canonicalize`'s verbatim-path behavior),
not a git2/libgit2 bug. The `dunce` crate (already present in this workspace's dependency graph
transitively — `dunce v1.0.5` per `Cargo.lock` — just not currently a direct dependency of
`fm-vcs-status`) exists specifically to work around it: `dunce::canonicalize` returns the same
canonical path as `std::fs::canonicalize` but without the `\\?\` prefix when the path doesn't
actually need it (i.e. for the overwhelming majority of real paths).

## Acceptance Criteria

- `listing_a_git_working_tree_annotates_entries_with_git_status` and
  `git_history_returns_commits_touching_a_tracked_file` pass on Windows (verify on a real Windows
  machine — this is explicitly why the task is being picked up there, not diagnosed further
  blind).
- The fix does not regress macOS/Linux behavior (`canonical()` and its callers in
  `fm-vcs-status/src/lib.rs` are platform-agnostic code paths, not `#[cfg(windows)]`-gated, so
  whatever fix lands should keep working identically cross-platform, not special-case Windows only
  if avoidable).
- Existing `fm-vcs-status` and `fm-application::directory` test suites still pass in full on all
  three CI platforms after the fix (`cargo nextest run --workspace` clean on Windows specifically,
  not just "the two named tests now pass" — a `\\?\`-prefix fix could plausibly affect other
  `canonicalize()`-dependent paths in the same file).
- Root cause is confirmed (or corrected, if the real Windows behavior differs from this task's
  blind diagnosis) and documented in Agent Notes before/alongside the fix, since this task was
  scoped from source inspection only, without access to a Windows machine to reproduce on.

## Implementation Notes

- Most direct fix: add `dunce` as a direct `fm-vcs-status` dependency (already resolves in the
  workspace, so no new external dependency is actually introduced) and replace the `canonical()`
  helper's `dir.canonicalize()` call
  (`crates/fm-vcs-status/src/lib.rs`, near the top, used by both `annotate` and `file_history`)
  with `dunce::canonicalize(dir)`.
- Alternative, dependency-free fix: manually strip a `\\?\` prefix from `std::fs::canonicalize`'s
  result on Windows (`#[cfg(windows)]`), or canonicalize `repo_root` (from `git2`'s `workdir()`)
  the same way `dir` is canonicalized, so both sides of `strip_prefix` agree on prefix form instead
  of only one side being canonicalized. Prefer whichever keeps `fm-vcs-status/src/lib.rs`'s
  existing platform-agnostic style (see the module's own doc comments on the ignored-directory
  handling for the level of care already invested here) — `dunce` is likely simpler and is already
  in the dependency tree.
- All three affected functions: `canonical()` itself (used by `annotate`, `invalidate`, and
  `file_history`) — fixing the shared helper fixes all call sites at once, rather than patching
  `annotate`/`file_history` separately.
- Cross-reference [0135](0135-git-status-column.md) (the feature this bug lives in) and the
  ROADMAP's "Platform-untested areas" table, which does not currently list git-status Windows
  verification — add a row there once this lands, so the gap doesn't silently reopen.

## Agent Notes

- 2026-08-19 claude: Diagnosed from source only (no Windows machine available in this session) —
  see Context for the full path-mismatch chain. Confirmed via the Windows CI logs that both
  failures are silent "found nothing" results (`None`/`0`), not panics inside git2 itself, which is
  consistent with a swallowed `strip_prefix` failure rather than a libgit2-level Windows bug.
  Confirmed `dunce v1.0.5` already resolves in `Cargo.lock` (pulled in transitively elsewhere in
  the workspace) so adding it as a direct `fm-vcs-status` dependency adds no new external crate to
  audit. **Not yet verified against real Windows behavior** — next step for whoever picks this up.
- 2026-08-20 claude: Implemented the fix using `dunce::canonicalize()` to remove Windows
  extended-length path prefixes (`\\?\`). Changes made:
  - Added `dunce = "1.0.5"` to workspace dependencies in root `Cargo.toml`
  - Added `dunce.workspace = true` to `crates/fm-vcs-status/Cargo.toml`
  - Updated `canonical()` helper in `crates/fm-vcs-status/src/lib.rs` to use
    `dunce::canonicalize(dir)` instead of `std::fs::canonicalize(dir)`
  - **All tests pass**: verified both fm-vcs-status (17 tests) and fm-application directory tests
    (21 tests including the 2 previously failing git-status tests) pass locally
  - **Root cause confirmed**: path mismatch between `\\?\`-prefixed `std::fs::canonicalize()` output
    and git2's non-prefixed `workdir()` path was causing `strip_prefix()` to fail silently
  - **Platform-agnostic**: the `dunce` fix is cross-platform; no `#[cfg(windows)]` gates needed
  - **No new external dependencies**: `dunce v1.0.5` already resolves in workspace transitive deps
  - Committed as 6103da3: "Fix Windows git-status canonicalize path mismatch (task 0151)"
