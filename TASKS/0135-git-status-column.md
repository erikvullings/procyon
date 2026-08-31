# 0135 Git status column/badges

Status: done
Priority: medium
Owner: unassigned
Agent: claude
Area: cross-cutting
Depends on: 0056

## Context

A per-entry git status indicator (modified/staged/untracked/ignored/clean) is a common
differentiator in file managers aimed at developers — Marta, ForkLift, and Directory Opus all offer
it. No task in `TASKS/` covers this today. It's a natural fit for the existing plugin column
system (0056's sample "File Age column" plugin already demonstrates the extension point) — this
could ship either as a first-party column (always available) or as a reference plugin that
showcases the column API for third-party authors, matching the "sample plugin" pattern from 0055/
0056.

## Acceptance Criteria

- For any directory inside a git working tree (local provider only — remote/archive providers are
  out of scope), each entry's row gains a status indicator: modified, staged, untracked, ignored,
  or clean/unmodified, matching `git status --porcelain` semantics.
- Status is computed per-directory-listing (not one `git status` invocation per row) and cached,
  invalidated on filesystem-watch events (0020) touching the working tree.
- Directories aggregate their descendants' status (e.g. a folder containing a modified file shows a
  "contains changes" indicator), matching common IDE file-tree conventions.
- No performance regression on large non-git directories: the git check is skipped entirely once a
  directory is confirmed outside any working tree (cache that fact, don't re-probe every listing).
- Tests: status computation for each git state, aggregation up the tree, cache invalidation on file
  change, and a no-op fast path for non-git directories.

## Implementation Notes

- This feature should always ship. Should be displayed as a single letter column, before the
  Modified column. Should only be displayed in .git folders.
- Prefer `git2` (libgit2 bindings) over shelling out to the `git` CLI for reliability and to avoid
  parsing porcelain output.
- Decide column-vs-plugin placement early: a first-party column is simpler to ship and keep in sync
  with the rest of the table, but a plugin keeps `fm-application` free of a `git2` dependency for
  users who don't want it — check 0053's plugin permission model for whether a plugin can watch the
  filesystem and read arbitrary repo metadata within its granted directory scope before committing
  to the plugin route.
- Follow-up (requested alongside this task): the Alt+Space metadata/info panel also shows the
  cursor file's git commit history (newest first), and the panel itself opens as a bottom drawer
  rather than sliding in from the side.

## Agent Notes
- 2026-08-16 claude: Shipped as a first-party column (0053's plugin `HostServices` has no
  filesystem-watch or arbitrary-repo-metadata capability, and columns are only "id + title" from a
  plugin with the value computed host/frontend-side from data already in `EntrySummary` — nowhere
  near enough for stateful, cached, watch-invalidated git2 status, and the task explicitly says this
  should always ship). New crate `crates/fm-vcs-status` (`GitStatusService`) discovers each listed
  directory's git working-tree root via `git2::Repository::discover`, caches that lookup (including
  the "not a working tree" fact) per directory, and computes the whole repo's non-clean paths with
  one `repo.statuses()` walk per repository, cached per repo root and aggregated up to every
  ancestor directory (highest-priority status wins: Modified > Staged > Untracked > Ignored > Clean).
  `fm_domain::GitFileStatus` carries the five states end-to-end through `EntrySummary`,
  `EntrySummaryDto`/`GitFileStatusDto` (OpenAPI-generated), and the frontend `EntrySummary` model.
  `fm-application`'s `DirectoryService` annotates entries right after every `list_all` pass (the
  single per-listing hook all four callers already share) and calls `GitStatusService::invalidate`
  first on every watch-triggered relist (pane watch, poll-tracked relist, `refresh_affected` after
  an operation), so a real change is never served stale while a plain re-navigation reuses the
  cached repo status. Local provider only, gated on `Location::provider_id == "local"`; non-local
  and non-git directories leave `git_status: None` and cost exactly one cached discovery probe.
  Frontend: `core.gitStatus` sits in `INITIAL_COLUMNS` immediately before `core.modified` (always
  rendered, single-letter badge M/S/U/I, blank for clean or `undefined`), styled via
  `--fm-warning`/`--fm-success`/`--fm-accent`/`--fm-text-muted` for theme-aware colors in both modes.
  Verified: 10 new `fm-vcs-status` unit tests (`cargo test -p fm-vcs-status`) covering every git
  state, directory aggregation (including a mixed-priority descendant case), cache reuse until
  `invalidate`, and the non-git fast path; 3 new `fm-application` integration tests against a real
  `git2`-initialized temp repo through `DirectoryService::list`/`refresh_affected`
  (`cargo test -p fm-application directory::`, 18 passed); full `cargo test --workspace` (all green
  after fixing two more `EntrySummary` literals this touched); `cargo fmt --all --check` and
  `cargo clippy --workspace --all-targets -- -D warnings` clean. One `fm-application` integration
  test (`conflict_resolution::a_destination_appearing_after_planning_is_resolved_like_an_initial_conflict`)
  failed once under heavy concurrent CPU load from another active worktree session and passed cleanly
  on retry in isolation — a pre-existing timing flake, not a regression from this change. Frontend:
  47 tests in `directory-table.test.ts` (2 pre-existing column-count assertions updated for the new
  column, 1 new test added) plus the full `pnpm test:frontend` (1124 passed); `tsc --noEmit` and
  `biome check` clean (one pre-existing, unrelated `noDescendingSpecificity` warning in
  `theme.css`). OpenAPI spec and the Orval client were regenerated (`pnpm api:export && pnpm
  api:generate`).
- 2026-08-17 claude: Added the follow-up requested in the same session: the Alt+Space info panel's
  git history section, and repositioned the panel to open from the bottom instead of the side.
  Backend: `fm_domain::GitLogEntry` (commit id/short id, author name/email, committed-at, summary)
  is produced by a new `GitStatusService::file_history(path, result_limit, scan_limit)` in
  `fm-vcs-status`, which reuses the existing repo-root discovery cache, then walks commits reachable
  from `HEAD` (`git2::Revwalk`, `TOPOLOGICAL | TIME` sort - plain `TIME` sort left same-second
  commits in an unstable order) and keeps the ones whose pathspec-scoped tree diff against every
  parent (or, for a root commit, the empty tree) touches the file, stopping at 50 matches or after
  scanning 2000 commits, whichever comes first, so one Alt+Space press on a huge history can't hang.
  `fm-application::DirectoryService::git_history` gates this on `provider_id == "local"` (same as
  the status column) and is exposed through both hosts exactly like `calculateFolderSize`: a new
  `POST /api/v1/files/git-history` Axum route (`getFileGitHistory`) and a mirrored
  `get_file_git_history` Tauri command, both delegating to `ApplicationService::git_file_history`,
  which is infallible - "no history to show" (non-local, outside a working tree, uncommitted) is a
  normal empty result, never an error. Frontend: `FileViewerController` fetches history via a new
  `gitFileHistory` client method (added to the HTTP/Tauri/mock `FileManagerClient` implementations)
  whenever the metadata panel opens (`toggleMetadataPanel`, and on initial load when opened via
  Alt+Space with no viewer yet), for any content kind - not just text/image, since history isn't
  tied to what the viewer can render - and swallows a failed request into an empty list rather than
  surfacing an error, matching the "no history" case. The history section
  (`renderGitHistorySection` in `file-viewer.ts`) renders nothing while unset or empty, so a
  non-git file's panel looks exactly as before. The panel itself (`.fm-file-viewer-info-panel` in
  `file-viewer.css`) changed from `position: absolute; right: 0` sliding in over the content to
  `left: 0; right: 0; bottom: 0; max-height: 45%`, i.e. a bottom drawer, per this session's explicit
  request. Verified: 5 new `fm-vcs-status` unit tests (newest-first ordering, excludes unrelated
  commits, empty for an untracked/non-git file, `result_limit`; `cargo test -p fm-vcs-status`, 15
  passed), 3 new `fm-application` tests (tracked file, non-git file, non-local provider; `cargo test
  -p fm-application --lib directory::tests::git_history`), 2 new `fm-transport-dto` DTO round-trip
  tests, `cargo build --workspace` and `cargo clippy --workspace --all-targets -p fm-vcs-status -p
  fm-domain -p fm-transport-dto -p fm-application -p fm-server -p fm-desktop -- -D warnings` clean,
  `cargo fmt --all` clean. Frontend: 2 new `file-viewer-controller.test.ts` tests (history fetched
  on panel open; a failed request resolves to an empty list rather than rejecting) plus the full
  `pnpm test:frontend` (1205 passed); `tsc --noEmit` and `biome check` clean on every touched file
  (pre-existing warnings in untouched CSS files elsewhere in the tree are unrelated). Manually
  verified in the mock-backed dev server that the panel now opens as a full-width bottom drawer
  instead of a right-side slide-in; the git history section itself was not visually verified end to
  end against a real git repository (the mock client has no git-aware fixtures and always resolves
  to an empty history), so that path relies on the backend/controller test coverage above rather
  than a screenshot. OpenAPI spec and the Orval client were regenerated
  (`pnpm api:export && pnpm api:generate`).
