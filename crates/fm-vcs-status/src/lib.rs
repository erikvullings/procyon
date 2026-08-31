//! Per-directory git working-tree status (task 0135), local provider only.
//!
//! [`GitStatusService`] discovers the git working tree (if any) that owns a
//! listed directory, computes each changed path's status with a single
//! [`git2`] status walk per repository, and caches both the repo-root lookup
//! and the computed status map. Directories that sit outside any working
//! tree are cached as such, so repeatedly listing a large non-git directory
//! tree never re-probes git2.
//!
//! Callers invalidate the cached status for a directory's repository (e.g.
//! on a filesystem-watch event) via [`GitStatusService::invalidate`]; the
//! next [`GitStatusService::annotate`] call recomputes it.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use fm_domain::{EntryKind, EntrySummary, GitFileStatus};

/// Computes and caches git working-tree status for directory listings.
#[derive(Default)]
pub struct GitStatusService {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    /// Listed directory -> the working tree root that owns it, or `None`
    /// when the directory is confirmed to sit outside any git working tree.
    repo_roots: HashMap<PathBuf, Option<PathBuf>>,
    /// Working tree root -> its most recently computed status.
    statuses: HashMap<PathBuf, Arc<RepoStatus>>,
}

#[derive(Default)]
struct RepoStatus {
    /// Path (relative to the repo root) -> status, for every non-clean file.
    /// Absent entries are clean (git2 only reports non-current paths).
    files: HashMap<PathBuf, GitFileStatus>,
    /// Path (relative to the repo root) -> aggregated status, for every
    /// ancestor directory of a non-clean file.
    dirs: HashMap<PathBuf, GitFileStatus>,
}

impl GitStatusService {
    /// Creates an empty service with nothing cached yet.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Annotates `entries` (the direct children of `dir`, a native
    /// filesystem path) with their git status.
    ///
    /// A no-op — every entry's `git_status` is left as-is — when `dir` sits
    /// outside any git working tree; that fact is cached, so a directory
    /// tree with no `.git` anywhere never triggers more than one discovery
    /// probe per listed directory.
    ///
    /// [`status_for`](Self::status_for) never walks into an ignored
    /// directory's contents (a repo's `target/`/`node_modules/` can hold
    /// hundreds of thousands of files — walking them eagerly on every
    /// listing took over a minute on this repo's own `target/` and blocked
    /// every other in-flight directory listing behind the shared git2 call).
    /// So an entry inside an ignored directory is never found in the cached
    /// status maps; for those, this falls back to a targeted
    /// `git2::Repository::status_should_ignore` pattern-match per entry —
    /// cheap (no directory walk) and only paid for entries actually shown.
    pub async fn annotate(&self, dir: &Path, entries: &mut [EntrySummary]) {
        let dir = canonical(dir);
        let Some(repo_root) = self.repo_root_for(&dir) else {
            return;
        };
        let Ok(rel_dir) = dir.strip_prefix(&repo_root) else {
            return;
        };
        let status = self.status_for(&repo_root).await;

        let mut resolved: Vec<Option<GitFileStatus>> = Vec::with_capacity(entries.len());
        let mut unresolved: Vec<(usize, PathBuf)> = Vec::new();
        for (index, entry) in entries.iter().enumerate() {
            let rel_path = if rel_dir.as_os_str().is_empty() {
                PathBuf::from(&entry.name)
            } else {
                rel_dir.join(&entry.name)
            };
            let found = match entry.kind {
                EntryKind::Directory => status.dirs.get(&rel_path).copied(),
                EntryKind::File | EntryKind::Symlink => status.files.get(&rel_path).copied(),
            };
            if found.is_none() {
                unresolved.push((index, rel_path));
            }
            resolved.push(found);
        }

        if !unresolved.is_empty() {
            let repo_root = repo_root.clone();
            let ignored =
                tokio::task::spawn_blocking(move || resolve_ignored(&repo_root, unresolved))
                    .await
                    .unwrap_or_default();
            for (index, is_ignored) in ignored {
                resolved[index] = Some(if is_ignored {
                    GitFileStatus::Ignored
                } else {
                    GitFileStatus::Clean
                });
            }
        }

        for (entry, status) in entries.iter_mut().zip(resolved) {
            entry.git_status = Some(status.unwrap_or(GitFileStatus::Clean));
        }
    }

    /// Drops the cached status for `dir`'s working tree, if one is cached,
    /// so the next [`Self::annotate`] call recomputes it. A no-op when
    /// `dir`'s repo-root membership has not been discovered yet (nothing is
    /// cached to invalidate; the next `annotate` call already computes it
    /// fresh).
    pub fn invalidate(&self, dir: &Path) {
        let dir = canonical(dir);
        let mut inner = self.inner.lock().expect("git status lock poisoned");
        if let Some(Some(root)) = inner.repo_roots.get(&dir).cloned() {
            inner.statuses.remove(&root);
        }
    }

    fn repo_root_for(&self, dir: &Path) -> Option<PathBuf> {
        {
            let inner = self.inner.lock().expect("git status lock poisoned");
            if let Some(cached) = inner.repo_roots.get(dir) {
                return cached.clone();
            }
        }
        let root = discover_repo_root(dir);
        let mut inner = self.inner.lock().expect("git status lock poisoned");
        inner.repo_roots.insert(dir.to_path_buf(), root.clone());
        root
    }

    async fn status_for(&self, repo_root: &Path) -> Arc<RepoStatus> {
        {
            let inner = self.inner.lock().expect("git status lock poisoned");
            if let Some(cached) = inner.statuses.get(repo_root) {
                return Arc::clone(cached);
            }
        }
        let owned_root = repo_root.to_path_buf();
        let computed = tokio::task::spawn_blocking(move || {
            compute_repo_status(&owned_root).unwrap_or_default()
        })
        .await
        .unwrap_or_default();
        let computed = Arc::new(computed);
        let mut inner = self.inner.lock().expect("git status lock poisoned");
        inner
            .statuses
            .insert(repo_root.to_path_buf(), Arc::clone(&computed));
        computed
    }

    /// Returns `path`'s commit history (newest first, matching `git log`) within its git working
    /// tree, or an empty vector when `path` is not in a git working tree, is unborn (no commits
    /// touch it yet), or the repository can't be opened. Scans at most `scan_limit` commits from
    /// `HEAD` looking for ones that touch `path`, and stops once `result_limit` matches are found,
    /// whichever comes first, so a huge, mostly-unrelated history never turns one Alt+Space press
    /// into an unbounded walk.
    #[must_use]
    pub async fn file_history(
        &self,
        path: &Path,
        result_limit: usize,
        scan_limit: usize,
    ) -> Vec<fm_domain::GitLogEntry> {
        let path = canonical(path);
        let Some(parent) = path.parent() else {
            return Vec::new();
        };
        let Some(repo_root) = self.repo_root_for(parent) else {
            return Vec::new();
        };
        let Ok(rel_path) = path.strip_prefix(&repo_root) else {
            return Vec::new();
        };
        let rel_path = rel_path.to_path_buf();
        tokio::task::spawn_blocking(move || {
            compute_file_history(&repo_root, &rel_path, result_limit, scan_limit)
                .unwrap_or_default()
        })
        .await
        .unwrap_or_default()
    }

    #[cfg(test)]
    fn repo_root_is_cached(&self, dir: &Path) -> bool {
        self.inner
            .lock()
            .expect("git status lock poisoned")
            .repo_roots
            .contains_key(&canonical(dir))
    }
}

/// Resolves symlinks (e.g. macOS's `/var` -> `/private/var` `TMPDIR`) so a
/// listed directory's path matches the realpath `git2` reports as a
/// repository's working directory. Falls back to the original path if it no
/// longer exists (e.g. it was just deleted).
/// Uses `dunce::canonicalize` to avoid Windows `\\?\` verbatim-path prefix issues.
fn canonical(dir: &Path) -> PathBuf {
    dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf())
}

/// Finds the working tree that owns `dir`, if any. Bare repositories (no
/// working directory) are treated as "no working tree" — there is nothing
/// to show a status for.
fn discover_repo_root(dir: &Path) -> Option<PathBuf> {
    let repo = git2::Repository::discover(dir).ok()?;
    repo.workdir().map(Path::to_path_buf)
}

/// Walks the whole repository's status once via `git2` and aggregates it
/// both per-file and per-ancestor-directory.
///
/// Deliberately does **not** set `recurse_ignored_dirs`: with it enabled,
/// this produced one status entry per file *inside* every ignored directory
/// (e.g. a Rust workspace's `target/`, or `node_modules/`), which on this
/// repo's own `target/` (1.5M+ build artifacts) took upwards of two minutes
/// and, since [`GitStatusService::status_for`] runs one call at a time under
/// its cache-fill path, stalled every other pane's directory listing behind
/// it too. `include_ignored(true)` still reports each ignored directory as a
/// single collapsed entry (so, e.g., `target/` itself is flagged ignored
/// without walking its contents); per-entry status for something *inside*
/// an ignored directory is instead resolved on demand by
/// [`GitStatusService::annotate`] via the cheap `status_should_ignore`
/// pattern-match, only for entries actually being listed.
fn compute_repo_status(repo_root: &Path) -> Option<RepoStatus> {
    let repo = git2::Repository::open(repo_root).ok()?;
    let mut options = git2::StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(true)
        .include_ignored(true);
    let statuses = repo.statuses(Some(&mut options)).ok()?;

    let mut files = HashMap::new();
    for entry in statuses.iter() {
        let Some(path) = entry.path() else { continue };
        files.insert(PathBuf::from(path), classify(entry.status()));
    }

    let mut dirs: HashMap<PathBuf, GitFileStatus> = HashMap::new();
    for (path, status) in &files {
        let mut ancestor = path.parent();
        while let Some(parent) = ancestor {
            if parent.as_os_str().is_empty() {
                break;
            }
            dirs.entry(parent.to_path_buf())
                .and_modify(|existing| {
                    if priority(*status) > priority(*existing) {
                        *existing = *status;
                    }
                })
                .or_insert(*status);
            ancestor = parent.parent();
        }
    }

    Some(RepoStatus { files, dirs })
}

/// Resolves whether each `(index, path relative to the repo root)` pair is
/// git-ignored via a targeted `status_should_ignore` pattern-match — a
/// single-path lookup, not a directory walk — used by
/// [`GitStatusService::annotate`] for entries [`compute_repo_status`]'s
/// non-recursive-into-ignored-dirs walk did not itself resolve. Opening the
/// repository fails only if it was removed since discovery; every entry is
/// then reported not-ignored rather than dropped; a per-path lookup failure
/// (also effectively never on a valid path) does the same.
fn resolve_ignored(repo_root: &Path, unresolved: Vec<(usize, PathBuf)>) -> Vec<(usize, bool)> {
    let Ok(repo) = git2::Repository::open(repo_root) else {
        return unresolved
            .into_iter()
            .map(|(index, _)| (index, false))
            .collect();
    };
    unresolved
        .into_iter()
        .map(|(index, rel_path)| {
            let ignored = repo.status_should_ignore(&rel_path).unwrap_or(false);
            (index, ignored)
        })
        .collect()
}

/// Walks commits reachable from `HEAD`, newest first, collecting the ones whose tree-diff against
/// each parent (or, for a root commit, against the empty tree) touches `rel_path`.
fn compute_file_history(
    repo_root: &Path,
    rel_path: &Path,
    result_limit: usize,
    scan_limit: usize,
) -> Option<Vec<fm_domain::GitLogEntry>> {
    let repo = git2::Repository::open(repo_root).ok()?;
    let mut revwalk = repo.revwalk().ok()?;
    revwalk.push_head().ok()?;
    // TOPOLOGICAL keeps parents after children even when several commits share the same
    // second-resolution author time (common in fast test fixtures and squashed history).
    revwalk
        .set_sorting(git2::Sort::TOPOLOGICAL | git2::Sort::TIME)
        .ok()?;

    let mut entries = Vec::new();
    for oid in revwalk.take(scan_limit) {
        if entries.len() >= result_limit {
            break;
        }
        let oid = oid.ok()?;
        let commit = repo.find_commit(oid).ok()?;
        if !commit_touches_path(&repo, &commit, rel_path) {
            continue;
        }
        let author = commit.author();
        let Some(committed_at) = chrono::DateTime::from_timestamp(commit.time().seconds(), 0)
        else {
            continue;
        };
        entries.push(fm_domain::GitLogEntry {
            commit_id: oid.to_string(),
            short_id: commit
                .as_object()
                .short_id()
                .ok()
                .and_then(|buf| buf.as_str().map(str::to_owned))
                .unwrap_or_else(|| oid.to_string()),
            author_name: author.name().unwrap_or("").to_owned(),
            author_email: author.email().unwrap_or("").to_owned(),
            committed_at,
            summary: commit.summary().unwrap_or("").to_owned(),
        });
    }
    Some(entries)
}

/// Whether `commit`'s tree differs from every parent's tree (or, for a root commit, the empty
/// tree) at `rel_path`. Uses a pathspec-scoped diff so only the one path's delta is computed,
/// not the whole tree.
fn commit_touches_path(repo: &git2::Repository, commit: &git2::Commit, rel_path: &Path) -> bool {
    let Ok(tree) = commit.tree() else {
        return false;
    };
    let mut diff_options = git2::DiffOptions::new();
    diff_options.pathspec(rel_path.to_string_lossy().as_ref());

    if commit.parent_count() == 0 {
        return repo
            .diff_tree_to_tree(None, Some(&tree), Some(&mut diff_options))
            .is_ok_and(|diff| diff.deltas().len() > 0);
    }
    for parent_index in 0..commit.parent_count() {
        let Ok(parent) = commit.parent(parent_index) else {
            continue;
        };
        let Ok(parent_tree) = parent.tree() else {
            continue;
        };
        let touched = repo
            .diff_tree_to_tree(Some(&parent_tree), Some(&tree), Some(&mut diff_options))
            .is_ok_and(|diff| diff.deltas().len() > 0);
        if touched {
            return true;
        }
    }
    false
}

/// Maps `git2`'s bitflags onto the single status the column shows,
/// preferring the change most likely to need the user's attention: an
/// unstaged working-tree edit outranks a staged one, which outranks an
/// untracked file, which outranks an ignored one.
fn classify(flags: git2::Status) -> GitFileStatus {
    use git2::Status;
    if flags.intersects(
        Status::WT_MODIFIED
            | Status::WT_DELETED
            | Status::WT_TYPECHANGE
            | Status::WT_RENAMED
            | Status::CONFLICTED,
    ) {
        GitFileStatus::Modified
    } else if flags.intersects(
        Status::INDEX_NEW
            | Status::INDEX_MODIFIED
            | Status::INDEX_DELETED
            | Status::INDEX_RENAMED
            | Status::INDEX_TYPECHANGE,
    ) {
        GitFileStatus::Staged
    } else if flags.contains(Status::WT_NEW) {
        GitFileStatus::Untracked
    } else if flags.contains(Status::IGNORED) {
        GitFileStatus::Ignored
    } else {
        GitFileStatus::Clean
    }
}

fn priority(status: GitFileStatus) -> u8 {
    match status {
        GitFileStatus::Modified => 4,
        GitFileStatus::Staged => 3,
        GitFileStatus::Untracked => 2,
        GitFileStatus::Ignored => 1,
        GitFileStatus::Clean => 0,
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::Duration;

    use fm_domain::{EntryId, EntryKind, GitFileStatus, Location, ProviderId};

    use super::*;

    fn entry(dir: &Path, name: &str, kind: EntryKind) -> EntrySummary {
        EntrySummary {
            id: EntryId::new(),
            location: Location::new(
                ProviderId::new("local"),
                format!("file://{}", dir.join(name).display()),
            ),
            name: name.to_owned(),
            kind,
            size: Some(0),
            modified_at: None,
            created_at: None,
            hidden: false,
            read_only: false,
            extension: None,
            mime_type: None,
            icon_key: None,
            metadata_revision: 0,
            git_status: None,
        }
    }

    fn init_repo(root: &Path) -> git2::Repository {
        let repo = git2::Repository::init(root).expect("init repo");
        {
            let mut config = repo.config().expect("repo config");
            config.set_str("user.name", "Test").expect("set name");
            config
                .set_str("user.email", "test@example.com")
                .expect("set email");
        }
        repo
    }

    fn commit_all(repo: &git2::Repository, message: &str) {
        let mut index = repo.index().expect("index");
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .expect("stage all");
        index.write().expect("write index");
        let tree_id = index.write_tree().expect("write tree");
        let tree = repo.find_tree(tree_id).expect("find tree");
        let signature = repo.signature().expect("signature");
        let parents: Vec<git2::Commit> = repo
            .head()
            .ok()
            .and_then(|head| head.peel_to_commit().ok())
            .into_iter()
            .collect();
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(
            Some("HEAD"),
            &signature,
            &signature,
            message,
            &tree,
            &parent_refs,
        )
        .expect("commit");
    }

    #[tokio::test]
    async fn non_git_directory_is_a_no_op_fast_path() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("plain.txt"), b"hello").expect("write file");

        let service = GitStatusService::new();
        let mut entries = vec![entry(dir.path(), "plain.txt", EntryKind::File)];
        service.annotate(dir.path(), &mut entries).await;

        assert_eq!(entries[0].git_status, None);
        assert!(service.repo_root_is_cached(dir.path()));

        // A second call must not re-probe git2 (there is nothing left on
        // disk to discover from) yet must still behave identically.
        fs::remove_dir_all(dir.path()).ok();
        let mut entries = vec![entry(dir.path(), "plain.txt", EntryKind::File)];
        service.annotate(dir.path(), &mut entries).await;
        assert_eq!(entries[0].git_status, None);
    }

    #[tokio::test]
    async fn clean_tracked_file_has_clean_status() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("a.txt"), b"a").expect("write file");
        let repo = init_repo(dir.path());
        commit_all(&repo, "initial");

        let service = GitStatusService::new();
        let mut entries = vec![entry(dir.path(), "a.txt", EntryKind::File)];
        service.annotate(dir.path(), &mut entries).await;

        assert_eq!(entries[0].git_status, Some(GitFileStatus::Clean));
    }

    #[tokio::test]
    async fn modified_tracked_file_is_reported_modified() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("a.txt"), b"a").expect("write file");
        let repo = init_repo(dir.path());
        commit_all(&repo, "initial");
        fs::write(dir.path().join("a.txt"), b"changed").expect("modify file");

        let service = GitStatusService::new();
        let mut entries = vec![entry(dir.path(), "a.txt", EntryKind::File)];
        service.annotate(dir.path(), &mut entries).await;

        assert_eq!(entries[0].git_status, Some(GitFileStatus::Modified));
    }

    #[tokio::test]
    async fn staged_file_is_reported_staged() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("a.txt"), b"a").expect("write file");
        let repo = init_repo(dir.path());
        commit_all(&repo, "initial");
        fs::write(dir.path().join("a.txt"), b"staged change").expect("modify file");
        let mut index = repo.index().expect("index");
        index.add_path(Path::new("a.txt")).expect("stage file");
        index.write().expect("write index");

        let service = GitStatusService::new();
        let mut entries = vec![entry(dir.path(), "a.txt", EntryKind::File)];
        service.annotate(dir.path(), &mut entries).await;

        assert_eq!(entries[0].git_status, Some(GitFileStatus::Staged));
    }

    #[tokio::test]
    async fn untracked_file_is_reported_untracked() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path());
        commit_all(&repo, "empty");
        fs::write(dir.path().join("new.txt"), b"new").expect("write file");

        let service = GitStatusService::new();
        let mut entries = vec![entry(dir.path(), "new.txt", EntryKind::File)];
        service.annotate(dir.path(), &mut entries).await;

        assert_eq!(entries[0].git_status, Some(GitFileStatus::Untracked));
    }

    #[tokio::test]
    async fn ignored_file_is_reported_ignored() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join(".gitignore"), b"ignored.txt\n").expect("write gitignore");
        let repo = init_repo(dir.path());
        commit_all(&repo, "add gitignore");
        fs::write(dir.path().join("ignored.txt"), b"ignore me").expect("write file");

        let service = GitStatusService::new();
        let mut entries = vec![entry(dir.path(), "ignored.txt", EntryKind::File)];
        service.annotate(dir.path(), &mut entries).await;

        assert_eq!(entries[0].git_status, Some(GitFileStatus::Ignored));
    }

    /// Regression test for the "entering a large repo's own directory stalls
    /// navigation for the whole app" bug: an ignored directory containing
    /// many files (standing in for `target/`/`node_modules/`) must resolve
    /// almost instantly — proving `compute_repo_status` no longer recurses
    /// into it — while still being correctly reported ignored itself.
    #[tokio::test]
    async fn a_large_ignored_directory_resolves_quickly_without_walking_its_contents() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join(".gitignore"), b"build/\n").expect("write gitignore");
        fs::write(dir.path().join("tracked.txt"), b"a").expect("write tracked file");
        let repo = init_repo(dir.path());
        commit_all(&repo, "initial");
        let build_dir = dir.path().join("build");
        fs::create_dir(&build_dir).expect("mkdir build");
        for index in 0..2_000 {
            fs::write(build_dir.join(format!("artifact-{index}.o")), b"binary")
                .expect("write build artifact");
        }

        let service = GitStatusService::new();
        let mut entries = vec![
            entry(dir.path(), "tracked.txt", EntryKind::File),
            entry(dir.path(), "build", EntryKind::Directory),
        ];

        let started = std::time::Instant::now();
        service.annotate(dir.path(), &mut entries).await;
        let elapsed = started.elapsed();

        assert_eq!(entries[0].git_status, Some(GitFileStatus::Clean));
        assert_eq!(entries[1].git_status, Some(GitFileStatus::Ignored));
        assert!(
            elapsed < Duration::from_secs(2),
            "annotating the parent directory must not walk into the ignored \
             directory's 2,000 files: took {elapsed:?}"
        );
    }

    /// A file actually browsed *inside* an ignored directory (e.g. the user
    /// opens `target/`) must still be reported ignored, even though
    /// `compute_repo_status`'s walk never enumerated it — this is what the
    /// `status_should_ignore` fallback in `annotate` is for.
    #[tokio::test]
    async fn a_file_inside_an_ignored_directory_is_still_reported_ignored_when_browsed_into() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join(".gitignore"), b"build/\n").expect("write gitignore");
        let repo = init_repo(dir.path());
        commit_all(&repo, "initial");
        let build_dir = dir.path().join("build");
        fs::create_dir(&build_dir).expect("mkdir build");
        fs::write(build_dir.join("artifact.o"), b"binary").expect("write build artifact");

        let service = GitStatusService::new();
        let mut entries = vec![entry(&build_dir, "artifact.o", EntryKind::File)];
        service.annotate(&build_dir, &mut entries).await;

        assert_eq!(entries[0].git_status, Some(GitFileStatus::Ignored));
    }

    #[tokio::test]
    async fn directory_aggregates_a_modified_descendant() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir(dir.path().join("sub")).expect("mkdir sub");
        fs::write(dir.path().join("sub/nested.txt"), b"a").expect("write nested");
        fs::write(dir.path().join("clean-sub2.txt"), b"b").expect("write sibling");
        let repo = init_repo(dir.path());
        commit_all(&repo, "initial");
        fs::write(dir.path().join("sub/nested.txt"), b"changed").expect("modify nested");

        let service = GitStatusService::new();
        let mut entries = vec![
            entry(dir.path(), "sub", EntryKind::Directory),
            entry(dir.path(), "clean-sub2.txt", EntryKind::File),
        ];
        service.annotate(dir.path(), &mut entries).await;

        assert_eq!(entries[0].git_status, Some(GitFileStatus::Modified));
        assert_eq!(entries[1].git_status, Some(GitFileStatus::Clean));
    }

    #[tokio::test]
    async fn directory_aggregate_prefers_highest_priority_descendant_status() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::create_dir(dir.path().join("sub")).expect("mkdir sub");
        fs::write(dir.path().join("sub/tracked.txt"), b"a").expect("write tracked");
        let repo = init_repo(dir.path());
        commit_all(&repo, "initial");
        // One descendant is merely untracked, the other is a modified
        // tracked file — the directory must reflect the higher-priority one.
        fs::write(dir.path().join("sub/tracked.txt"), b"changed").expect("modify tracked");
        fs::write(dir.path().join("sub/untracked.txt"), b"new").expect("add untracked");

        let service = GitStatusService::new();
        let mut entries = vec![entry(dir.path(), "sub", EntryKind::Directory)];
        service.annotate(dir.path(), &mut entries).await;

        assert_eq!(entries[0].git_status, Some(GitFileStatus::Modified));
    }

    #[tokio::test]
    async fn cached_status_is_reused_until_invalidated() {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("a.txt"), b"a").expect("write file");
        let repo = init_repo(dir.path());
        commit_all(&repo, "initial");

        let service = GitStatusService::new();
        let mut entries = vec![entry(dir.path(), "a.txt", EntryKind::File)];
        service.annotate(dir.path(), &mut entries).await;
        assert_eq!(entries[0].git_status, Some(GitFileStatus::Clean));

        fs::write(dir.path().join("a.txt"), b"changed").expect("modify file");

        // Without invalidation the stale, cached status is served.
        let mut entries = vec![entry(dir.path(), "a.txt", EntryKind::File)];
        service.annotate(dir.path(), &mut entries).await;
        assert_eq!(entries[0].git_status, Some(GitFileStatus::Clean));

        service.invalidate(dir.path());

        let mut entries = vec![entry(dir.path(), "a.txt", EntryKind::File)];
        service.annotate(dir.path(), &mut entries).await;
        assert_eq!(entries[0].git_status, Some(GitFileStatus::Modified));
    }

    #[tokio::test]
    async fn invalidating_an_unknown_directory_is_a_no_op() {
        let dir = tempfile::tempdir().expect("tempdir");
        let service = GitStatusService::new();
        service.invalidate(dir.path());
    }

    #[tokio::test]
    async fn file_history_returns_commits_touching_the_file_newest_first() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.txt");
        fs::write(&file, b"one").expect("write v1");
        let repo = init_repo(dir.path());
        commit_all(&repo, "first");
        fs::write(&file, b"two").expect("write v2");
        commit_all(&repo, "second");

        let service = GitStatusService::new();
        let history = service.file_history(&file, 10, 100).await;

        assert_eq!(history.len(), 2);
        assert_eq!(history[0].summary, "second");
        assert_eq!(history[1].summary, "first");
        assert_eq!(history[0].commit_id.len(), 40);
        assert!(!history[0].short_id.is_empty());
        assert_eq!(history[0].author_name, "Test");
        assert_eq!(history[0].author_email, "test@example.com");
    }

    #[tokio::test]
    async fn file_history_excludes_commits_that_did_not_touch_the_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("target.txt");
        let other = dir.path().join("other.txt");
        fs::write(&target, b"a").expect("write target");
        fs::write(&other, b"b").expect("write other");
        let repo = init_repo(dir.path());
        commit_all(&repo, "initial");
        fs::write(&other, b"changed").expect("modify other only");
        commit_all(&repo, "unrelated change");

        let service = GitStatusService::new();
        let history = service.file_history(&target, 10, 100).await;

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].summary, "initial");
    }

    #[tokio::test]
    async fn file_history_of_an_untracked_file_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let repo = init_repo(dir.path());
        commit_all(&repo, "empty");
        let file = dir.path().join("new.txt");
        fs::write(&file, b"new").expect("write file");

        let service = GitStatusService::new();
        let history = service.file_history(&file, 10, 100).await;

        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn file_history_of_a_non_git_directory_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("plain.txt");
        fs::write(&file, b"hello").expect("write file");

        let service = GitStatusService::new();
        let history = service.file_history(&file, 10, 100).await;

        assert!(history.is_empty());
    }

    #[tokio::test]
    async fn file_history_respects_result_limit() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("a.txt");
        let repo = init_repo(dir.path());
        for revision in 0..5 {
            fs::write(&file, format!("v{revision}")).expect("write revision");
            commit_all(&repo, &format!("revision {revision}"));
        }

        let service = GitStatusService::new();
        let history = service.file_history(&file, 2, 100).await;

        assert_eq!(history.len(), 2);
        assert_eq!(history[0].summary, "revision 4");
        assert_eq!(history[1].summary, "revision 3");
    }
}
