//! Include/exclude path filters for sync planning (task 0161).
//!
//! A saved synchronization profile narrows which compared paths may appear in
//! its plan. Filtering is pure and runs on already-computed comparison results,
//! so it can only ever *remove* proposed actions, never add or change one.
//!
//! Pattern rules (case-sensitive, `/`-separated relative paths):
//! - `*` matches any run of characters except `/`; `**` also crosses `/`; `?` matches one
//!   character except `/`.
//! - A pattern without `/` is matched against every path segment (so `node_modules` or `*.tmp`
//!   apply at any depth).
//! - A pattern with `/` is matched against the whole path from the compared root.
//! - A path is matched when it, or any of its ancestor directories, matches; excluding a
//!   directory therefore excludes everything below it.

use crate::model::ComparisonEntry;
use crate::sync::{SyncMode, SyncPlanItem, generate_sync_plan};

/// Compiled include/exclude pattern sets.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathFilter {
    include: Vec<String>,
    exclude: Vec<String>,
}

impl PathFilter {
    /// Builds a filter; blank patterns are ignored. An empty include list admits every path.
    #[must_use]
    pub fn new(include: &[String], exclude: &[String]) -> Self {
        let clean = |patterns: &[String]| {
            patterns
                .iter()
                .map(|pattern| pattern.trim().trim_start_matches("./").to_owned())
                .filter(|pattern| !pattern.is_empty())
                .collect::<Vec<_>>()
        };
        Self {
            include: clean(include),
            exclude: clean(exclude),
        }
    }

    /// Whether `relative_path` survives the filter. Exclusions always win over inclusions.
    #[must_use]
    pub fn admits(&self, relative_path: &str) -> bool {
        if self
            .exclude
            .iter()
            .any(|pattern| pattern_matches_path(pattern, relative_path))
        {
            return false;
        }
        self.include.is_empty()
            || self
                .include
                .iter()
                .any(|pattern| pattern_matches_path(pattern, relative_path))
    }

    /// True when the filter admits everything.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty()
    }
}

/// Proposes a sync plan from the entries a [`PathFilter`] admits.
#[must_use]
pub fn generate_filtered_sync_plan(
    entries: &[ComparisonEntry],
    mode: SyncMode,
    filter: &PathFilter,
) -> Vec<SyncPlanItem> {
    if filter.is_empty() {
        return generate_sync_plan(entries, mode);
    }
    let admitted: Vec<ComparisonEntry> = entries
        .iter()
        .filter(|entry| filter.admits(&entry.relative_path))
        .cloned()
        .collect();
    generate_sync_plan(&admitted, mode)
}

fn pattern_matches_path(pattern: &str, relative_path: &str) -> bool {
    let whole_path = pattern.contains('/');
    let segments: Vec<&str> = relative_path.split('/').collect();
    (1..=segments.len()).any(|end| {
        if whole_path {
            glob_match(pattern, &segments[..end].join("/"))
        } else {
            glob_match(pattern, segments[end - 1])
        }
    })
}

/// Iterative glob matcher over `char`s: `**` crosses `/`, `*` and `?` do not.
fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    matches_from(&pattern, &text)
}

fn matches_from(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => {
            let crosses = rest.first() == Some(&'*');
            let rest = if crosses { &rest[1..] } else { rest };
            (0..=text.len()).any(|taken| {
                if !crosses && text[..taken].contains(&'/') {
                    return false;
                }
                matches_from(rest, &text[taken..])
            })
        }
        Some(('?', rest)) => {
            matches!(text.first(), Some(c) if *c != '/') && matches_from(rest, &text[1..])
        }
        Some((expected, rest)) => text.first() == Some(expected) && matches_from(rest, &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{ComparisonEntrySide, ComparisonStatus};
    use fm_domain::EntryKind;

    fn filter(include: &[&str], exclude: &[&str]) -> PathFilter {
        let own = |items: &[&str]| items.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        PathFilter::new(&own(include), &own(exclude))
    }

    fn only_left(path: &str) -> ComparisonEntry {
        ComparisonEntry {
            relative_path: path.to_owned(),
            left: Some(ComparisonEntrySide {
                kind: EntryKind::File,
                size: Some(1),
                modified_at: None,
                content_hash: None,
            }),
            right: None,
            status: ComparisonStatus::OnlyLeft,
        }
    }

    #[test]
    fn empty_filter_admits_everything() {
        assert!(filter(&[], &[]).admits("a/b.txt"));
        assert!(filter(&["  "], &[""]).is_empty());
    }

    #[test]
    fn segment_patterns_apply_at_any_depth_and_exclude_descendants() {
        let f = filter(&[], &["node_modules", "*.tmp"]);
        assert!(!f.admits("node_modules"));
        assert!(!f.admits("pkg/node_modules/x/y.js"));
        assert!(!f.admits("deep/dir/scratch.tmp"));
        assert!(f.admits("src/main.rs"));
    }

    #[test]
    fn slash_patterns_match_from_the_root_and_double_star_crosses_directories() {
        let f = filter(&["docs/**/*.md"], &[]);
        assert!(f.admits("docs/a/b/readme.md"));
        assert!(!f.admits("other/docs/readme.md"));
        assert!(!f.admits("docs/readme.txt"));
        let single = filter(&["docs/*.md"], &[]);
        assert!(single.admits("docs/readme.md"));
        assert!(!single.admits("docs/a/readme.md"));
        assert!(filter(&["a?c"], &[]).admits("abc"));
        assert!(!filter(&["a?c"], &[]).admits("a/c"));
    }

    #[test]
    fn exclusions_win_over_inclusions() {
        let f = filter(&["*.rs"], &["generated"]);
        assert!(f.admits("src/lib.rs"));
        assert!(!f.admits("generated/out.rs"));
        assert!(!f.admits("src/readme.md"));
    }

    #[test]
    fn filtered_plan_only_removes_proposals_from_unsorted_input() {
        let entries = vec![
            only_left("z/keep.txt"),
            only_left("build/out.bin"),
            only_left("a/keep.txt"),
            only_left("note.tmp"),
        ];
        let plan = generate_filtered_sync_plan(
            &entries,
            SyncMode::MirrorLeftToRight,
            &filter(&[], &["build", "*.tmp"]),
        );
        let paths: Vec<_> = plan.iter().map(|i| i.relative_path.as_str()).collect();
        assert_eq!(paths, ["z/keep.txt", "a/keep.txt"]);
        let unfiltered = generate_filtered_sync_plan(
            &entries,
            SyncMode::MirrorLeftToRight,
            &PathFilter::default(),
        );
        assert_eq!(unfiltered.len(), 4);
    }
}
