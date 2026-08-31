//! Reading, writing and verifying checksum files (task 0077).
//!
//! # Format
//!
//! The GNU coreutils layout used by `sha256sum`, `md5sum` and friends is
//! adopted verbatim so files written here can be verified with the standard
//! tools, and files produced by those tools can be verified here:
//!
//! ```text
//! <lower-case hex digest><space><space><path>
//! ```
//!
//! Exactly two spaces separate the digest from the path. Coreutils writes a
//! `*` in place of the second space's following character for *binary* mode
//! (`<digest><space>*<path>`); reading accepts that form and records it as
//! [`ChecksumFileEntry::binary`], while writing always emits the textual
//! two-space form because every file this application hashes is read as raw
//! bytes and the distinction is meaningless outside legacy platforms.
//!
//! Blank lines and `#` comment lines are skipped, as coreutils does. The file
//! carries no algorithm marker of its own — coreutils infers it from the tool
//! name and the digest width — so [`write_checksum_file`] takes the algorithm
//! explicitly and [`read_checksum_file`] infers it per line from the digest
//! width where that is unambiguous.

use std::io::{BufRead, Write};

use serde::{Deserialize, Serialize};

use crate::error::ChecksumFileError;
use crate::hash::ChecksumAlgorithm;

/// One `<digest>  <path>` line of a checksum file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChecksumFileEntry {
    /// Path as written in the file, normally relative to the file's directory.
    pub path: String,
    /// Lower-case hex digest recorded for that path.
    pub digest: String,
    /// Whether the line used coreutils' binary-mode `*` marker.
    pub binary: bool,
    /// Algorithm inferred from the digest width, when it is unambiguous.
    ///
    /// SHA-256 and BLAKE3 share a 64-character width, so a 64-character
    /// digest yields `None` and the caller must supply the algorithm.
    pub algorithm: Option<ChecksumAlgorithm>,
}

/// The outcome of verifying one checksum-file entry against a computed digest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "kebab-case")]
pub enum VerificationStatus {
    /// The computed digest equals the recorded one.
    Match,
    /// The file exists but hashes differently.
    Mismatch {
        /// Digest recorded in the checksum file.
        expected: String,
        /// Digest computed from the file on disk.
        actual: String,
    },
    /// The checksum file lists a path that was not found among the computed
    /// digests.
    Missing,
}

/// One verified path together with its outcome.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerificationResult {
    /// Path exactly as recorded in the checksum file.
    pub path: String,
    /// What verifying that path produced.
    pub status: VerificationStatus,
}

impl VerificationResult {
    /// Whether this entry verified successfully.
    #[must_use]
    pub const fn is_match(&self) -> bool {
        matches!(self.status, VerificationStatus::Match)
    }
}

/// Infers the algorithm from a digest's hex width, where it is unambiguous.
fn algorithm_for_width(hex_len: usize) -> Option<ChecksumAlgorithm> {
    match hex_len {
        8 => Some(ChecksumAlgorithm::Crc32),
        32 => Some(ChecksumAlgorithm::Md5),
        // 64 is shared by SHA-256 and BLAKE3, so it stays ambiguous.
        _ => None,
    }
}

/// Writes `entries` in the coreutils `<digest>  <path>` format.
///
/// `algorithm` is recorded only as a leading `# <name>` comment: coreutils
/// carries no in-band algorithm field, and a comment keeps the file readable
/// by `sha256sum --check` while preserving the provenance for this
/// application.
///
/// # Errors
///
/// Returns [`ChecksumFileError::Io`] if the writer fails.
pub fn write_checksum_file<W, P>(
    entries: &[(P, String)],
    algorithm: ChecksumAlgorithm,
    mut writer: W,
) -> Result<(), ChecksumFileError>
where
    W: Write,
    P: AsRef<str>,
{
    writeln!(writer, "# {algorithm}")?;
    for (path, digest) in entries {
        writeln!(writer, "{digest}  {}", path.as_ref())?;
    }
    Ok(())
}

/// Parses a checksum file into entries, skipping blank and comment lines.
///
/// # Errors
///
/// Returns [`ChecksumFileError::MalformedLine`] for a line without the
/// two-space separator, [`ChecksumFileError::MalformedDigest`] for a
/// non-hexadecimal or odd-length digest, and [`ChecksumFileError::Io`] if the
/// reader fails.
pub fn read_checksum_file<R>(reader: R) -> Result<Vec<ChecksumFileEntry>, ChecksumFileError>
where
    R: BufRead,
{
    let mut entries = Vec::new();
    for (index, line) in reader.lines().enumerate() {
        let line = line?;
        let number = index + 1;
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.trim().is_empty() || trimmed.starts_with('#') {
            continue;
        }
        entries.push(parse_line(trimmed, number)?);
    }
    Ok(entries)
}

fn parse_line(line: &str, number: usize) -> Result<ChecksumFileEntry, ChecksumFileError> {
    let (digest, remainder) = line
        .split_once(' ')
        .ok_or(ChecksumFileError::MalformedLine { line: number })?;
    // Coreutils writes one separating space followed by a mode marker: a
    // second space for text mode, `*` for binary mode.
    let (binary, path) = match remainder.strip_prefix('*') {
        Some(path) => (true, path),
        None => (
            false,
            remainder
                .strip_prefix(' ')
                .ok_or(ChecksumFileError::MalformedLine { line: number })?,
        ),
    };
    if path.is_empty() {
        return Err(ChecksumFileError::MalformedLine { line: number });
    }
    if digest.is_empty()
        || !digest.len().is_multiple_of(2)
        || !digest
            .chars()
            .all(|character| character.is_ascii_hexdigit())
    {
        return Err(ChecksumFileError::MalformedDigest { line: number });
    }
    Ok(ChecksumFileEntry {
        path: path.to_owned(),
        digest: digest.to_ascii_lowercase(),
        binary,
        algorithm: algorithm_for_width(digest.len()),
    })
}

/// Compares recorded digests against digests computed from disk.
///
/// `computed` maps a path — spelled exactly as the checksum file spells it —
/// to the digest calculated for it. A checksum-file entry with no matching
/// computed digest is reported as [`VerificationStatus::Missing`], which is
/// also how a caller reports a file it could not open: it simply omits the
/// path from `computed`.
///
/// Digest comparison is case-insensitive, since coreutils accepts either case.
#[must_use]
pub fn verify<'a, I>(computed: I, recorded: &[ChecksumFileEntry]) -> Vec<VerificationResult>
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let computed: std::collections::HashMap<&str, &str> = computed.into_iter().collect();
    recorded
        .iter()
        .map(|entry| {
            let status = match computed.get(entry.path.as_str()) {
                None => VerificationStatus::Missing,
                Some(actual) if actual.eq_ignore_ascii_case(&entry.digest) => {
                    VerificationStatus::Match
                }
                Some(actual) => VerificationStatus::Mismatch {
                    expected: entry.digest.clone(),
                    actual: (*actual).to_owned(),
                },
            };
            VerificationResult {
                path: entry.path.clone(),
                status,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infers_the_algorithm_from_unambiguous_digest_widths() {
        assert_eq!(algorithm_for_width(8), Some(ChecksumAlgorithm::Crc32));
        assert_eq!(algorithm_for_width(32), Some(ChecksumAlgorithm::Md5));
        assert_eq!(algorithm_for_width(64), None);
        assert_eq!(algorithm_for_width(7), None);
    }

    #[test]
    fn rejects_a_line_without_the_two_space_separator() {
        let error = parse_line("abcd file.txt", 4).expect_err("must reject");
        assert!(matches!(
            error,
            ChecksumFileError::MalformedLine { line: 4 }
        ));
    }

    #[test]
    fn rejects_a_non_hex_digest() {
        let error = parse_line("zzzz  file.txt", 2).expect_err("must reject");
        assert!(matches!(
            error,
            ChecksumFileError::MalformedDigest { line: 2 }
        ));
    }

    #[test]
    fn keeps_spaces_inside_a_path() {
        let entry =
            parse_line("d41d8cd98f00b204e9800998ecf8427e  my file.txt", 1).expect("must parse");
        assert_eq!(entry.path, "my file.txt");
        assert!(!entry.binary);
        assert_eq!(entry.algorithm, Some(ChecksumAlgorithm::Md5));
    }

    #[test]
    fn records_the_binary_mode_marker() {
        let entry =
            parse_line("d41d8cd98f00b204e9800998ecf8427e *image.bin", 1).expect("must parse");
        assert_eq!(entry.path, "image.bin");
        assert!(entry.binary);
    }
}
