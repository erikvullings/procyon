//! A short `git status` of the working tree that owns a folder: the branch, how far it is from its
//! upstream, and the changed paths with porcelain-style codes. Used to tell the terminal assistant
//! what state the repository is in.

use std::path::Path;

/// One changed path, relative to the repository root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedPath {
    /// Two letters as `git status --short` shows them: the index, then the working tree
    /// (`M `, ` M`, `A `, `D `, `R `, `??`, `UU`).
    pub code: String,
    /// The path, relative to the repository root; an untracked folder ends with `/`.
    pub path: String,
}

/// The state of a working tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkingTreeSummary {
    /// The checked-out branch; `None` on a detached head or before the first commit.
    pub branch: Option<String>,
    /// Commits ahead of and behind the branch's upstream, when it has one.
    pub ahead_behind: Option<(usize, usize)>,
    /// Changed paths, sorted, at most the number asked for.
    pub changes: Vec<ChangedPath>,
    /// Changed paths left out of `changes`.
    pub more: usize,
}

/// The status of the working tree that owns `dir`, listing at most `max_paths` changed paths.
/// `None` outside a working tree. Untracked folders are shown once (`dir/`) and ignored files
/// not at all, so a large build folder is never walked.
#[must_use]
pub fn working_tree_summary(dir: &Path, max_paths: usize) -> Option<WorkingTreeSummary> {
    let repo = git2::Repository::discover(dir).ok()?;
    repo.workdir()?;
    let mut options = git2::StatusOptions::new();
    options
        .include_untracked(true)
        .recurse_untracked_dirs(false)
        .include_ignored(false)
        .exclude_submodules(true)
        .renames_head_to_index(true);
    let statuses = repo.statuses(Some(&mut options)).ok()?;
    let mut changes: Vec<ChangedPath> = statuses
        .iter()
        .filter_map(|entry| {
            let path = entry.path().ok()?.to_owned();
            let code = short_code(entry.status())?;
            Some(ChangedPath { code, path })
        })
        .collect();
    changes.sort_by(|a, b| a.path.cmp(&b.path));
    let more = changes.len().saturating_sub(max_paths);
    changes.truncate(max_paths);
    let head = repo.head().ok();
    let branch = head
        .as_ref()
        .filter(|head| head.is_branch())
        .and_then(|head| head.shorthand().ok().map(str::to_owned));
    let ahead_behind = branch.as_deref().and_then(|name| {
        let local = repo.find_branch(name, git2::BranchType::Local).ok()?;
        let upstream = local.upstream().ok()?;
        let ours = local.get().target()?;
        let theirs = upstream.get().target()?;
        repo.graph_ahead_behind(ours, theirs).ok()
    });
    Some(WorkingTreeSummary {
        branch,
        ahead_behind,
        changes,
        more,
    })
}

/// The `git status --short` letters for one entry; `None` for a clean or ignored one.
fn short_code(flags: git2::Status) -> Option<String> {
    use git2::Status;
    if flags.contains(Status::CONFLICTED) {
        return Some("UU".to_owned());
    }
    if flags.contains(Status::WT_NEW) {
        return Some("??".to_owned());
    }
    let index = if flags.contains(Status::INDEX_NEW) {
        'A'
    } else if flags.contains(Status::INDEX_MODIFIED) {
        'M'
    } else if flags.contains(Status::INDEX_DELETED) {
        'D'
    } else if flags.contains(Status::INDEX_RENAMED) {
        'R'
    } else if flags.contains(Status::INDEX_TYPECHANGE) {
        'T'
    } else {
        ' '
    };
    let tree = if flags.contains(Status::WT_MODIFIED) {
        'M'
    } else if flags.contains(Status::WT_DELETED) {
        'D'
    } else if flags.contains(Status::WT_RENAMED) {
        'R'
    } else if flags.contains(Status::WT_TYPECHANGE) {
        'T'
    } else {
        ' '
    };
    (index != ' ' || tree != ' ').then(|| format!("{index}{tree}"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn commit_all(repo: &git2::Repository) {
        let mut index = repo.index().unwrap();
        index
            .add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None)
            .unwrap();
        index.write().unwrap();
        let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
        let signature = git2::Signature::now("Test", "test@example.com").unwrap();
        let parents: Vec<git2::Commit> = repo
            .head()
            .ok()
            .and_then(|head| head.peel_to_commit().ok())
            .into_iter()
            .collect();
        let parents: Vec<&git2::Commit> = parents.iter().collect();
        repo.commit(Some("HEAD"), &signature, &signature, "c", &tree, &parents)
            .unwrap();
    }

    #[test]
    fn lists_staged_modified_and_untracked_paths_without_walking_ignored_ones() {
        let root = tempfile::tempdir().unwrap();
        let repo = git2::Repository::init(root.path()).unwrap();
        fs::write(root.path().join(".gitignore"), "target/\n").unwrap();
        fs::write(root.path().join("a.txt"), "a").unwrap();
        fs::write(root.path().join("b.txt"), "b").unwrap();
        commit_all(&repo);

        fs::write(root.path().join("a.txt"), "changed").unwrap();
        fs::write(root.path().join("c.txt"), "new").unwrap();
        let mut index = repo.index().unwrap();
        index.add_path(Path::new("c.txt")).unwrap();
        index.write().unwrap();
        fs::create_dir_all(root.path().join("notes/deep")).unwrap();
        fs::write(root.path().join("notes/deep/x.md"), "x").unwrap();
        fs::create_dir_all(root.path().join("target")).unwrap();
        fs::write(root.path().join("target/out.o"), "o").unwrap();

        let summary = working_tree_summary(&root.path().join("notes"), 10).unwrap();
        let codes: Vec<(&str, &str)> = summary
            .changes
            .iter()
            .map(|change| (change.code.as_str(), change.path.as_str()))
            .collect();
        assert_eq!(
            codes,
            [(" M", "a.txt"), ("A ", "c.txt"), ("??", "notes/")],
            "an untracked folder is one entry and the ignored one is absent"
        );
        assert_eq!(summary.more, 0);
        assert!(summary.branch.is_some());
        assert_eq!(summary.ahead_behind, None, "no upstream");

        let capped = working_tree_summary(root.path(), 1).unwrap();
        assert_eq!(capped.changes.len(), 1);
        assert_eq!(capped.more, 2);
    }

    #[test]
    fn a_folder_outside_a_working_tree_has_no_summary() {
        let root = tempfile::tempdir().unwrap();
        assert_eq!(working_tree_summary(root.path(), 10), None);
    }
}
