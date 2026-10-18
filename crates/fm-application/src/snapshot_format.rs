//! The folder snapshot file format and the pure comparison rules (task 0268).
//!
//! A snapshot is UTF-8 text, one JSON object per line:
//!
//! ```text
//! {"format":"procyon-snapshot","version":1,"root":"file:///...","createdAt":"...","algorithm":"sha256","entryCount":2}
//! {"p":"docs","k":"d"}
//! {"p":"docs/a.txt","k":"f","s":120,"m":1700000000000,"h":"<sha256 hex>"}
//! ```
//!
//! `p` is the path relative to the recorded root (`/` separated, decoded), `k` is `f` (file) or
//! `d` (directory), `s` the size in bytes, `m` the modification time in milliseconds since the
//! Unix epoch, and `h` the optional SHA-256. Unknown fields are ignored, so later versions can
//! add fields; a `version` newer than [`SNAPSHOT_FORMAT_VERSION`] is rejected.

use serde::{Deserialize, Serialize};

/// Value of the header's `format` field.
pub(crate) const SNAPSHOT_FORMAT_NAME: &str = "procyon-snapshot";
/// Highest format version this build reads and the one it writes.
pub(crate) const SNAPSHOT_FORMAT_VERSION: u32 = 1;
/// Modification times closer than this (milliseconds) are equal: FAT stores two-second ticks.
pub(crate) const MODIFIED_TOLERANCE_MS: i64 = 2_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SnapshotHeader {
    pub(crate) format: String,
    pub(crate) version: u32,
    pub(crate) root: String,
    pub(crate) created_at: String,
    pub(crate) algorithm: Option<String>,
    pub(crate) entry_count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct SnapshotEntry {
    #[serde(rename = "p")]
    pub(crate) path: String,
    #[serde(rename = "k")]
    pub(crate) kind: String,
    #[serde(rename = "s", default, skip_serializing_if = "Option::is_none")]
    pub(crate) size: Option<u64>,
    #[serde(rename = "m", default, skip_serializing_if = "Option::is_none")]
    pub(crate) modified_ms: Option<i64>,
    #[serde(rename = "h", default, skip_serializing_if = "Option::is_none")]
    pub(crate) sha256: Option<String>,
}

impl SnapshotEntry {
    pub(crate) fn is_directory(&self) -> bool {
        self.kind == "d"
    }
}

/// Why a snapshot file could not be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub(crate) enum SnapshotFormatError {
    #[error("the file is empty")]
    Empty,
    #[error("this is not a Procyon snapshot")]
    NotASnapshot,
    #[error("the snapshot was written by a newer version of Procyon (format {0})")]
    NewerVersion(u32),
    #[error("line {line} is not a valid snapshot entry")]
    BadEntry { line: usize },
}

/// Serialises a snapshot: the header line followed by one line per entry.
pub(crate) fn render_snapshot(header: &SnapshotHeader, entries: &[SnapshotEntry]) -> String {
    let mut output = String::new();
    output.push_str(&serde_json::to_string(header).unwrap_or_default());
    output.push('\n');
    for entry in entries {
        output.push_str(&serde_json::to_string(entry).unwrap_or_default());
        output.push('\n');
    }
    output
}

/// Parses a snapshot file.
pub(crate) fn parse_snapshot(
    text: &str,
) -> Result<(SnapshotHeader, Vec<SnapshotEntry>), SnapshotFormatError> {
    let mut lines = text
        .lines()
        .enumerate()
        .filter(|(_, line)| !line.trim().is_empty());
    let (_, first) = lines.next().ok_or(SnapshotFormatError::Empty)?;
    let header: SnapshotHeader =
        serde_json::from_str(first).map_err(|_| SnapshotFormatError::NotASnapshot)?;
    if header.format != SNAPSHOT_FORMAT_NAME {
        return Err(SnapshotFormatError::NotASnapshot);
    }
    if header.version > SNAPSHOT_FORMAT_VERSION {
        return Err(SnapshotFormatError::NewerVersion(header.version));
    }
    let mut entries = Vec::new();
    for (index, line) in lines {
        let entry: SnapshotEntry = serde_json::from_str(line)
            .map_err(|_| SnapshotFormatError::BadEntry { line: index + 1 })?;
        if entry.kind != "f" && entry.kind != "d" {
            return Err(SnapshotFormatError::BadEntry { line: index + 1 });
        }
        entries.push(entry);
    }
    Ok((header, entries))
}

/// What the entry currently on disk looks like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CurrentState {
    pub(crate) is_directory: bool,
    pub(crate) size: u64,
    pub(crate) modified_ms: Option<i64>,
}

/// Outcome of comparing one recorded entry with the entry now at the same path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Comparison {
    Unchanged,
    Changed,
    /// Size and time cannot decide: hash the current file and compare with the recorded digest.
    /// `time_matches` tells whether a differing digest means corruption (time matches) or an
    /// ordinary change (time differs).
    NeedsChecksum {
        time_matches: bool,
    },
}

fn times_match(recorded: Option<i64>, current: Option<i64>) -> bool {
    match (recorded, current) {
        (Some(left), Some(right)) => (left - right).abs() < MODIFIED_TOLERANCE_MS,
        (None, None) => true,
        _ => false,
    }
}

/// Decides how a recorded entry relates to what is on disk now.
pub(crate) fn compare_entry(
    recorded: &SnapshotEntry,
    current: &CurrentState,
    verify_checksums: bool,
) -> Comparison {
    if recorded.is_directory() != current.is_directory {
        return Comparison::Changed;
    }
    if current.is_directory {
        return Comparison::Unchanged;
    }
    if recorded.size.is_some_and(|size| size != current.size) {
        return Comparison::Changed;
    }
    let time_matches = times_match(recorded.modified_ms, current.modified_ms);
    if verify_checksums && recorded.sha256.is_some() {
        return Comparison::NeedsChecksum { time_matches };
    }
    if time_matches {
        Comparison::Unchanged
    } else {
        Comparison::Changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(entry_count: u64) -> SnapshotHeader {
        SnapshotHeader {
            format: SNAPSHOT_FORMAT_NAME.to_owned(),
            version: SNAPSHOT_FORMAT_VERSION,
            root: "file:///data".to_owned(),
            created_at: "2026-10-18T10:00:00+00:00".to_owned(),
            algorithm: Some("sha256".to_owned()),
            entry_count,
        }
    }

    fn file(path: &str, size: u64, modified: i64, hash: Option<&str>) -> SnapshotEntry {
        SnapshotEntry {
            path: path.to_owned(),
            kind: "f".to_owned(),
            size: Some(size),
            modified_ms: Some(modified),
            sha256: hash.map(str::to_owned),
        }
    }

    #[test]
    fn round_trips_through_text() {
        let entries = vec![
            SnapshotEntry {
                path: "docs".to_owned(),
                kind: "d".to_owned(),
                size: None,
                modified_ms: None,
                sha256: None,
            },
            file("docs/ä b.txt", 12, 1_700_000_000_000, Some("ab")),
        ];
        let text = render_snapshot(&header(2), &entries);
        assert_eq!(text.lines().count(), 3);
        let (parsed_header, parsed_entries) = parse_snapshot(&text).expect("parse");
        assert_eq!(parsed_header, header(2));
        assert_eq!(parsed_entries, entries);
    }

    #[test]
    fn ignores_unknown_fields_and_rejects_foreign_or_newer_files() {
        let text = format!(
            "{}\n{}\n",
            r#"{"format":"procyon-snapshot","version":1,"root":"r","createdAt":"t","algorithm":null,"entryCount":1,"extra":true}"#,
            r#"{"p":"a","k":"f","s":1,"future":"x"}"#
        );
        let (_, entries) = parse_snapshot(&text).expect("tolerant parse");
        assert_eq!(entries.len(), 1);

        assert_eq!(parse_snapshot(""), Err(SnapshotFormatError::Empty));
        assert_eq!(
            parse_snapshot("hello"),
            Err(SnapshotFormatError::NotASnapshot)
        );
        assert_eq!(
            parse_snapshot(
                r#"{"format":"other","version":1,"root":"","createdAt":"","entryCount":0}"#
            ),
            Err(SnapshotFormatError::NotASnapshot)
        );
        assert_eq!(
            parse_snapshot(
                r#"{"format":"procyon-snapshot","version":9,"root":"","createdAt":"","entryCount":0}"#
            ),
            Err(SnapshotFormatError::NewerVersion(9))
        );
        let bad = format!(
            "{}\n{{\"p\":\"a\",\"k\":\"x\"}}\n",
            render_snapshot(&header(0), &[]).trim()
        );
        assert_eq!(
            parse_snapshot(&bad),
            Err(SnapshotFormatError::BadEntry { line: 2 })
        );
    }

    #[test]
    fn comparison_uses_size_then_time_then_checksum() {
        let recorded = file("a", 10, 10_000, Some("h"));
        let same = CurrentState {
            is_directory: false,
            size: 10,
            modified_ms: Some(10_500),
        };
        assert_eq!(
            compare_entry(&recorded, &same, true),
            Comparison::NeedsChecksum { time_matches: true }
        );
        assert_eq!(
            compare_entry(&recorded, &same, false),
            Comparison::Unchanged
        );

        let touched = CurrentState {
            modified_ms: Some(99_000),
            ..same
        };
        assert_eq!(
            compare_entry(&recorded, &touched, true),
            Comparison::NeedsChecksum {
                time_matches: false
            }
        );
        assert_eq!(
            compare_entry(&recorded, &touched, false),
            Comparison::Changed
        );

        let grown = CurrentState { size: 11, ..same };
        assert_eq!(compare_entry(&recorded, &grown, true), Comparison::Changed);
    }

    #[test]
    fn kind_changes_and_directories_are_handled() {
        let recorded = file("a", 10, 1, None);
        let directory = CurrentState {
            is_directory: true,
            size: 0,
            modified_ms: None,
        };
        assert_eq!(
            compare_entry(&recorded, &directory, true),
            Comparison::Changed
        );
        let recorded_dir = SnapshotEntry {
            path: "d".to_owned(),
            kind: "d".to_owned(),
            size: None,
            modified_ms: None,
            sha256: None,
        };
        assert_eq!(
            compare_entry(&recorded_dir, &directory, true),
            Comparison::Unchanged
        );
    }
}
