//! Comparison value objects (spec §16 milestone 5, §37, task 0075).
//!
//! Kept as plain, provider-neutral data plus a pure classification function
//! so a future "compare against a remote provider" need touches only the
//! traversal in [`crate::engine`], never these types (spec §6).

use chrono::{DateTime, Utc};
use fm_domain::EntryKind;
use serde::{Deserialize, Serialize};

/// How two directory trees are compared (spec §16 milestone 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ComparisonCriteria {
    /// Only whether an entry exists at the same relative path on both sides.
    NameOnly,
    /// Size and modification time, without reading file content.
    SizeAndTimestamp,
    /// A streamed content hash. Task 0077 shares this implementation once it
    /// lands (see the implementation note on the task).
    ContentHash,
}

/// Per-entry comparison outcome (spec §16 milestone 5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ComparisonStatus {
    /// Present only under the left root.
    OnlyLeft,
    /// Present only under the right root.
    OnlyRight,
    /// Present on both sides; the left entry is the more recently modified.
    Newer,
    /// Present on both sides; the left entry is the less recently modified.
    Older,
    /// Present on both sides with the same kind, but they differ (by size,
    /// or in content-hash mode by content) without a determinable
    /// newer/older direction.
    DifferentSize,
    /// Present on both sides and considered equal under the active
    /// criteria.
    Identical,
    /// Present on both sides at the same relative path but as different
    /// kinds (a file across from a directory).
    TypeMismatch,
}

/// One side's metadata for a compared entry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComparisonEntrySide {
    /// File, directory or symlink.
    pub kind: EntryKind,
    /// Size in bytes, when known and meaningful.
    pub size: Option<u64>,
    /// Last modification time, when reported by the provider.
    pub modified_at: Option<DateTime<Utc>>,
    /// Streamed content hash, populated only under
    /// [`ComparisonCriteria::ContentHash`] and only for regular files.
    pub content_hash: Option<String>,
}

/// One compared path, relative to both roots (spec §16 milestone 5).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComparisonEntry {
    /// Path relative to both roots, using `/` separators regardless of
    /// platform.
    pub relative_path: String,
    /// Metadata on the left side, absent when [`ComparisonStatus::OnlyRight`].
    pub left: Option<ComparisonEntrySide>,
    /// Metadata on the right side, absent when [`ComparisonStatus::OnlyLeft`].
    pub right: Option<ComparisonEntrySide>,
    /// The computed outcome for this path.
    pub status: ComparisonStatus,
}

/// Modification-time difference below which two timestamps are treated as
/// equal, absorbing filesystem timestamp-granularity noise (for example
/// FAT32's 2-second resolution) rather than reporting spurious differences.
const TIMESTAMP_TOLERANCE_SECONDS: i64 = 2;

/// Classifies two same-relative-path sides that are both present.
///
/// Presence-only outcomes ([`ComparisonStatus::OnlyLeft`] /
/// [`ComparisonStatus::OnlyRight`]) are decided by the traversal in
/// [`crate::engine`] before this function is ever called, since they need no
/// classification logic of their own.
#[must_use]
pub fn classify(
    left: &ComparisonEntrySide,
    right: &ComparisonEntrySide,
    criteria: ComparisonCriteria,
) -> ComparisonStatus {
    if left.kind != right.kind {
        return ComparisonStatus::TypeMismatch;
    }
    // A matched directory pair is always "identical" in itself: its
    // difference is entirely delegated to its (separately compared)
    // children, and a directory's own size/timestamp is not a meaningful
    // content signal.
    if left.kind == EntryKind::Directory {
        return ComparisonStatus::Identical;
    }
    match criteria {
        ComparisonCriteria::NameOnly => ComparisonStatus::Identical,
        ComparisonCriteria::SizeAndTimestamp => classify_by_size_and_timestamp(left, right),
        ComparisonCriteria::ContentHash => classify_by_content_hash(left, right),
    }
}

fn classify_by_size_and_timestamp(
    left: &ComparisonEntrySide,
    right: &ComparisonEntrySide,
) -> ComparisonStatus {
    if let Some(direction) = timestamp_direction(left, right) {
        return direction;
    }
    if left.size != right.size {
        return ComparisonStatus::DifferentSize;
    }
    ComparisonStatus::Identical
}

fn classify_by_content_hash(
    left: &ComparisonEntrySide,
    right: &ComparisonEntrySide,
) -> ComparisonStatus {
    match (&left.content_hash, &right.content_hash) {
        (Some(left_hash), Some(right_hash)) if left_hash == right_hash => {
            ComparisonStatus::Identical
        }
        (Some(_), Some(_)) => {
            // Content differs. Prefer a directional signal from timestamps
            // when available so the same left/right arrows work regardless
            // of criteria; otherwise fall back to a size difference, and
            // finally to `DifferentSize` as the generic "differs without an
            // orderable direction" bucket (for example same size, same
            // timestamp, different content).
            timestamp_direction(left, right).unwrap_or(ComparisonStatus::DifferentSize)
        }
        // No hash was computed on at least one side (symlinks, or a
        // directory routed here defensively): fall back to size/timestamp.
        _ => classify_by_size_and_timestamp(left, right),
    }
}

fn timestamp_direction(
    left: &ComparisonEntrySide,
    right: &ComparisonEntrySide,
) -> Option<ComparisonStatus> {
    let (left_time, right_time) = (left.modified_at?, right.modified_at?);
    let diff = left_time - right_time;
    if diff.num_seconds().abs() < TIMESTAMP_TOLERANCE_SECONDS {
        return None;
    }
    Some(if left_time > right_time {
        ComparisonStatus::Newer
    } else {
        ComparisonStatus::Older
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn side(
        kind: EntryKind,
        size: Option<u64>,
        modified_at: Option<DateTime<Utc>>,
    ) -> ComparisonEntrySide {
        ComparisonEntrySide {
            kind,
            size,
            modified_at,
            content_hash: None,
        }
    }

    fn at(seconds: i64) -> DateTime<Utc> {
        Utc.timestamp_opt(1_700_000_000 + seconds, 0).unwrap()
    }

    #[test]
    fn type_mismatch_wins_over_any_criteria() {
        let left = side(EntryKind::File, Some(1), Some(at(0)));
        let right = side(EntryKind::Directory, None, Some(at(0)));
        for criteria in [
            ComparisonCriteria::NameOnly,
            ComparisonCriteria::SizeAndTimestamp,
            ComparisonCriteria::ContentHash,
        ] {
            assert_eq!(
                classify(&left, &right, criteria),
                ComparisonStatus::TypeMismatch
            );
        }
    }

    #[test]
    fn matched_directories_are_always_identical() {
        let left = side(EntryKind::Directory, None, Some(at(0)));
        let right = side(EntryKind::Directory, None, Some(at(1000)));
        assert_eq!(
            classify(&left, &right, ComparisonCriteria::SizeAndTimestamp),
            ComparisonStatus::Identical
        );
    }

    #[test]
    fn name_only_ignores_size_and_timestamp_differences() {
        let left = side(EntryKind::File, Some(1), Some(at(0)));
        let right = side(EntryKind::File, Some(999), Some(at(999)));
        assert_eq!(
            classify(&left, &right, ComparisonCriteria::NameOnly),
            ComparisonStatus::Identical
        );
    }

    #[test]
    fn size_and_timestamp_reports_newer_when_left_is_more_recent() {
        let left = side(EntryKind::File, Some(10), Some(at(100)));
        let right = side(EntryKind::File, Some(10), Some(at(0)));
        assert_eq!(
            classify(&left, &right, ComparisonCriteria::SizeAndTimestamp),
            ComparisonStatus::Newer
        );
    }

    #[test]
    fn size_and_timestamp_reports_older_when_left_is_less_recent() {
        let left = side(EntryKind::File, Some(10), Some(at(0)));
        let right = side(EntryKind::File, Some(10), Some(at(100)));
        assert_eq!(
            classify(&left, &right, ComparisonCriteria::SizeAndTimestamp),
            ComparisonStatus::Older
        );
    }

    #[test]
    fn size_and_timestamp_treats_sub_tolerance_timestamp_noise_as_equal() {
        let left = side(EntryKind::File, Some(10), Some(at(0)));
        let right = side(EntryKind::File, Some(10), Some(at(1)));
        assert_eq!(
            classify(&left, &right, ComparisonCriteria::SizeAndTimestamp),
            ComparisonStatus::Identical
        );
    }

    #[test]
    fn size_and_timestamp_reports_different_size_when_timestamps_are_equal() {
        let left = side(EntryKind::File, Some(10), Some(at(0)));
        let right = side(EntryKind::File, Some(20), Some(at(0)));
        assert_eq!(
            classify(&left, &right, ComparisonCriteria::SizeAndTimestamp),
            ComparisonStatus::DifferentSize
        );
    }

    #[test]
    fn size_and_timestamp_reports_different_size_when_timestamps_are_unknown() {
        let left = side(EntryKind::File, Some(10), None);
        let right = side(EntryKind::File, Some(20), None);
        assert_eq!(
            classify(&left, &right, ComparisonCriteria::SizeAndTimestamp),
            ComparisonStatus::DifferentSize
        );
    }

    #[test]
    fn size_and_timestamp_reports_identical_when_both_size_and_timestamp_match() {
        let left = side(EntryKind::File, Some(10), Some(at(0)));
        let right = side(EntryKind::File, Some(10), Some(at(0)));
        assert_eq!(
            classify(&left, &right, ComparisonCriteria::SizeAndTimestamp),
            ComparisonStatus::Identical
        );
    }

    #[test]
    fn content_hash_reports_identical_when_hashes_match() {
        let mut left = side(EntryKind::File, Some(10), Some(at(0)));
        left.content_hash = Some("abc".to_owned());
        let mut right = side(EntryKind::File, Some(10), Some(at(1000)));
        right.content_hash = Some("abc".to_owned());
        assert_eq!(
            classify(&left, &right, ComparisonCriteria::ContentHash),
            ComparisonStatus::Identical
        );
    }

    #[test]
    fn content_hash_prefers_timestamp_direction_when_hashes_differ() {
        let mut left = side(EntryKind::File, Some(10), Some(at(100)));
        left.content_hash = Some("abc".to_owned());
        let mut right = side(EntryKind::File, Some(20), Some(at(0)));
        right.content_hash = Some("xyz".to_owned());
        assert_eq!(
            classify(&left, &right, ComparisonCriteria::ContentHash),
            ComparisonStatus::Newer
        );
    }

    #[test]
    fn content_hash_falls_back_to_different_size_without_a_timestamp_direction() {
        let mut left = side(EntryKind::File, Some(10), None);
        left.content_hash = Some("abc".to_owned());
        let mut right = side(EntryKind::File, Some(20), None);
        right.content_hash = Some("xyz".to_owned());
        assert_eq!(
            classify(&left, &right, ComparisonCriteria::ContentHash),
            ComparisonStatus::DifferentSize
        );
    }

    #[test]
    fn content_hash_without_a_computed_hash_falls_back_to_size_and_timestamp() {
        // Symlinks are never hashed; both sides keep `content_hash: None`.
        let left = side(EntryKind::Symlink, Some(10), Some(at(0)));
        let right = side(EntryKind::Symlink, Some(10), Some(at(0)));
        assert_eq!(
            classify(&left, &right, ComparisonCriteria::ContentHash),
            ComparisonStatus::Identical
        );
    }
}
