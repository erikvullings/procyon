//! Provider-neutral streaming content search and binary sniffing (task 0088).
//!
//! [`search_content`] scans any [`tokio::io::AsyncRead`] line-by-line without ever buffering the
//! whole file, so it works uniformly for every provider that supports [`crate::ProviderCapabilities::READ`]
//! (no [`crate::ProviderCapabilities::RANDOM_ACCESS`] required). It is intentionally generic over
//! the reader rather than tied to [`crate::FileSystemProvider`] so a future directory-wide content
//! search (task 0089) can reuse it per matched file.

use regex::bytes::Regex;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio_util::sync::CancellationToken;

use crate::VfsError;

/// Bytes read per underlying `AsyncRead::read` call.
const READ_CHUNK_BYTES: usize = 64 * 1024;

/// Safety cap on how many bytes accumulate before a line delimiter (`\n`) is found. Pathological
/// single-line files (e.g. minified content) are split at this boundary as a synthetic line break
/// rather than growing the buffer unbounded; the synthetic split does not advance `line_number`.
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// How often (in underlying reads) cancellation is polled.
const CANCELLATION_CHECK_INTERVAL_READS: u32 = 64;

/// UTF-16/UTF-32 byte-order marks. Files with these leading bytes are legitimately text (e.g.
/// Windows-authored `.xml`/`.xsd` are often saved as UTF-16) even though every ASCII-range
/// character is followed (or preceded) by a NUL byte, so they must be exempted from the NUL-byte
/// heuristic below rather than misdetected as binary.
const UTF16_LE_BOM: &[u8] = &[0xFF, 0xFE];
const UTF16_BE_BOM: &[u8] = &[0xFE, 0xFF];
const UTF32_LE_BOM: &[u8] = &[0xFF, 0xFE, 0x00, 0x00];
const UTF32_BE_BOM: &[u8] = &[0x00, 0x00, 0xFE, 0xFF];

/// Sniffs whether a byte sample looks like non-text (binary) content.
///
/// Uses the same NUL-byte heuristic convention as other file managers (spec/task 0088): text-like
/// content essentially never contains a NUL byte, while most binary formats do within their first
/// bytes. A leading UTF-16/UTF-32 BOM is exempted from this check since that encoding legitimately
/// puts a NUL byte around every ASCII-range character.
#[must_use]
pub fn looks_like_binary(sample: &[u8]) -> bool {
    if sample.starts_with(UTF32_LE_BOM)
        || sample.starts_with(UTF32_BE_BOM)
        || sample.starts_with(UTF16_LE_BOM)
        || sample.starts_with(UTF16_BE_BOM)
    {
        return false;
    }
    sample.contains(&0)
}

/// A validated, ready-to-run content-search query.
#[derive(Debug, Clone)]
pub enum ContentQuery {
    /// Plain substring search. Case-insensitive matching folds ASCII letters only (documented
    /// limitation: non-ASCII case folding is not attempted, which keeps byte offsets exact since
    /// ASCII case-folding never changes a byte's length, unlike full Unicode case folding).
    Substring {
        /// The literal text to search for.
        needle: String,
        /// Whether comparison is case-sensitive.
        case_sensitive: bool,
        /// Whether a match must be flanked by non-word bytes (or line start/end); uses an
        /// ASCII-only word-byte definition (alphanumeric or `_`), consistent with the ASCII-only
        /// case folding documented above.
        whole_word: bool,
    },
    /// A pre-validated regular expression, matched independently against each line's raw bytes.
    Regex(Box<Regex>),
}

/// A query string that failed validation before any I/O started.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ContentQueryError {
    /// The query was empty after trimming.
    #[error("search query must not be empty")]
    EmptyQuery,
    /// The regular expression failed to compile.
    #[error("invalid regular expression: {0}")]
    InvalidPattern(String),
}

impl ContentQuery {
    /// Validates and builds a query, mirroring the "regex opt-in and validated before use"
    /// convention already used for multi-rename (task 0072).
    pub fn new(
        query: &str,
        regex: bool,
        case_sensitive: bool,
        whole_word: bool,
    ) -> Result<Self, ContentQueryError> {
        if query.is_empty() {
            return Err(ContentQueryError::EmptyQuery);
        }
        if regex {
            let pattern = if case_sensitive {
                query.to_owned()
            } else {
                format!("(?i){query}")
            };
            let pattern = if whole_word {
                format!(r"\b(?:{pattern})\b")
            } else {
                pattern
            };
            Regex::new(&pattern)
                .map(|regex| Self::Regex(Box::new(regex)))
                .map_err(|error| ContentQueryError::InvalidPattern(error.to_string()))
        } else {
            Ok(Self::Substring {
                needle: query.to_owned(),
                case_sensitive,
                whole_word,
            })
        }
    }

    /// Returns the non-overlapping `(start, end)` byte ranges within `line` that match.
    fn find_all(&self, line: &[u8]) -> Vec<(usize, usize)> {
        match self {
            Self::Substring {
                needle,
                case_sensitive,
                whole_word,
            } => find_substring_matches(line, needle.as_bytes(), *case_sensitive, *whole_word),
            Self::Regex(regex) => regex
                .find_iter(line)
                .map(|m| (m.start(), m.end()))
                .collect(),
        }
    }
}

/// ASCII-only word-byte definition used by whole-word substring matching (see
/// [`ContentQuery::Substring`]'s `whole_word` docs).
fn is_word_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn find_substring_matches(
    haystack: &[u8],
    needle: &[u8],
    case_sensitive: bool,
    whole_word: bool,
) -> Vec<(usize, usize)> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    let mut start = 0;
    while start + needle.len() <= haystack.len() {
        let window = &haystack[start..start + needle.len()];
        let is_match = if case_sensitive {
            window == needle
        } else {
            window.eq_ignore_ascii_case(needle)
        };
        if is_match {
            let end = start + needle.len();
            let boundary_ok = !whole_word
                || ((start == 0 || !is_word_byte(haystack[start - 1]))
                    && (end == haystack.len() || !is_word_byte(haystack[end])));
            if boundary_ok {
                matches.push((start, end));
            }
            start += needle.len();
        } else {
            start += 1;
        }
    }
    matches
}

/// One content-search match location.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContentMatch {
    /// 1-based line number the match was found on.
    pub line_number: u64,
    /// Absolute byte offset (from the start of the file) of the match's first byte.
    pub match_start: u64,
    /// Length of the match in bytes.
    pub match_len: u32,
}

/// Result of a bounded content search.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ContentSearchOutcome {
    /// Matches found, in file order, up to `max_matches`.
    pub matches: Vec<ContentMatch>,
    /// `true` when `max_matches` was reached before the reader was exhausted.
    pub truncated: bool,
}

/// Scans `reader` for `query`, never buffering more than [`MAX_LINE_BYTES`] at once.
///
/// Stops early once `max_matches` matches are collected (`truncated` is then `true`), or when
/// `cancellation` is triggered (returns [`VfsError::Cancelled`]).
pub async fn search_content<R>(
    mut reader: R,
    query: &ContentQuery,
    max_matches: usize,
    cancellation: &CancellationToken,
) -> Result<ContentSearchOutcome, VfsError>
where
    R: AsyncRead + Unpin,
{
    let mut matches = Vec::new();
    let mut truncated = false;
    let mut carry: Vec<u8> = Vec::new();
    let mut file_offset: u64 = 0;
    let mut line_number: u64 = 1;
    let mut read_buf = vec![0_u8; READ_CHUNK_BYTES];
    let mut reads_since_check: u32 = 0;

    'outer: loop {
        let read = reader
            .read(&mut read_buf)
            .await
            .map_err(|error| VfsError::Io {
                message: error.to_string(),
            })?;
        let eof = read == 0;
        carry.extend_from_slice(&read_buf[..read]);

        loop {
            if let Some(pos) = carry.iter().position(|&byte| byte == b'\n') {
                let mut line: Vec<u8> = carry.drain(..=pos).collect();
                line.pop(); // drop the '\n' itself
                if line.last() == Some(&b'\r') {
                    line.pop();
                }
                let line_len_with_terminator = pos as u64 + 1;
                scan_line(
                    &line,
                    file_offset,
                    line_number,
                    query,
                    max_matches,
                    &mut matches,
                    &mut truncated,
                );
                file_offset += line_len_with_terminator;
                line_number += 1;
                if truncated {
                    break 'outer;
                }
            } else if carry.len() >= MAX_LINE_BYTES {
                let chunk_len = carry.len() as u64;
                let chunk = std::mem::take(&mut carry);
                scan_line(
                    &chunk,
                    file_offset,
                    line_number,
                    query,
                    max_matches,
                    &mut matches,
                    &mut truncated,
                );
                file_offset += chunk_len;
                if truncated {
                    break 'outer;
                }
            } else {
                break;
            }
        }

        if eof {
            break;
        }

        reads_since_check += 1;
        if reads_since_check >= CANCELLATION_CHECK_INTERVAL_READS {
            reads_since_check = 0;
            if cancellation.is_cancelled() {
                return Err(VfsError::Cancelled);
            }
        }
    }

    if !truncated && !carry.is_empty() {
        scan_line(
            &carry,
            file_offset,
            line_number,
            query,
            max_matches,
            &mut matches,
            &mut truncated,
        );
    }

    Ok(ContentSearchOutcome { matches, truncated })
}

#[allow(clippy::too_many_arguments)]
fn scan_line(
    line: &[u8],
    line_start_offset: u64,
    line_number: u64,
    query: &ContentQuery,
    max_matches: usize,
    matches: &mut Vec<ContentMatch>,
    truncated: &mut bool,
) {
    for (start, end) in query.find_all(line) {
        if matches.len() >= max_matches {
            *truncated = true;
            return;
        }
        matches.push(ContentMatch {
            line_number,
            match_start: line_start_offset + start as u64,
            match_len: (end - start) as u32,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(text: &str) -> ContentQuery {
        ContentQuery::new(text, false, false, false).expect("valid substring query")
    }

    #[tokio::test]
    async fn finds_matches_across_multiple_lines() {
        let content = b"the quick brown fox\njumps over\nthe lazy fox\n".to_vec();
        let outcome = search_content(
            content.as_slice(),
            &query("fox"),
            100,
            &CancellationToken::new(),
        )
        .await
        .expect("search must succeed");
        assert_eq!(outcome.matches.len(), 2);
        assert_eq!(outcome.matches[0].line_number, 1);
        assert_eq!(outcome.matches[0].match_start, 16);
        assert_eq!(outcome.matches[1].line_number, 3);
        assert!(!outcome.truncated);
    }

    #[tokio::test]
    async fn matches_on_a_final_line_without_a_trailing_newline() {
        let content = b"alpha\nbeta".to_vec();
        let outcome = search_content(
            content.as_slice(),
            &query("beta"),
            100,
            &CancellationToken::new(),
        )
        .await
        .expect("search must succeed");
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].line_number, 2);
        assert_eq!(outcome.matches[0].match_start, 6);
    }

    #[tokio::test]
    async fn case_insensitive_search_folds_ascii_only() {
        let content = b"Hello World\n".to_vec();
        let outcome = search_content(
            content.as_slice(),
            &query("WORLD"),
            100,
            &CancellationToken::new(),
        )
        .await
        .expect("search must succeed");
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].match_start, 6);
    }

    #[tokio::test]
    async fn case_sensitive_search_rejects_a_different_case() {
        let content = b"Hello World\n".to_vec();
        let query = ContentQuery::new("world", false, true, false).expect("valid query");
        let outcome = search_content(content.as_slice(), &query, 100, &CancellationToken::new())
            .await
            .expect("search must succeed");
        assert!(outcome.matches.is_empty());
    }

    #[tokio::test]
    async fn regex_query_matches_per_line() {
        let content = b"cat\ncar\ncow\n".to_vec();
        let regex_query = ContentQuery::new(r"^ca", true, true, false).expect("valid regex");
        let outcome = search_content(
            content.as_slice(),
            &regex_query,
            100,
            &CancellationToken::new(),
        )
        .await
        .expect("search must succeed");
        assert_eq!(outcome.matches.len(), 2);
        assert_eq!(outcome.matches[0].line_number, 1);
        assert_eq!(outcome.matches[1].line_number, 2);
    }

    #[tokio::test]
    async fn invalid_regex_is_rejected_before_any_scan() {
        let error = ContentQuery::new("(unterminated", true, true, false).expect_err("must reject");
        assert!(matches!(error, ContentQueryError::InvalidPattern(_)));
    }

    #[tokio::test]
    async fn empty_query_is_rejected() {
        let error = ContentQuery::new("", false, false, false).expect_err("must reject");
        assert_eq!(error, ContentQueryError::EmptyQuery);
    }

    #[tokio::test]
    async fn whole_word_substring_search_rejects_a_match_inside_a_larger_word() {
        let content = b"cat concatenate cats\n".to_vec();
        let whole_word_query =
            ContentQuery::new("cat", false, false, true).expect("valid substring query");
        let outcome = search_content(
            content.as_slice(),
            &whole_word_query,
            100,
            &CancellationToken::new(),
        )
        .await
        .expect("search must succeed");
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].match_start, 0);
    }

    #[tokio::test]
    async fn whole_word_regex_search_rejects_a_match_inside_a_larger_word() {
        let content = b"cat concatenate cats\n".to_vec();
        let whole_word_query =
            ContentQuery::new("cat", true, true, true).expect("valid regex query");
        let outcome = search_content(
            content.as_slice(),
            &whole_word_query,
            100,
            &CancellationToken::new(),
        )
        .await
        .expect("search must succeed");
        assert_eq!(outcome.matches.len(), 1);
        assert_eq!(outcome.matches[0].match_start, 0);
    }

    #[tokio::test]
    async fn search_stops_and_reports_truncation_at_max_matches() {
        let content = b"a\na\na\na\n".to_vec();
        let outcome = search_content(
            content.as_slice(),
            &query("a"),
            2,
            &CancellationToken::new(),
        )
        .await
        .expect("search must succeed");
        assert_eq!(outcome.matches.len(), 2);
        assert!(outcome.truncated);
    }

    #[tokio::test]
    async fn cancellation_stops_a_long_running_search() {
        let mut content = Vec::new();
        for _ in 0..(CANCELLATION_CHECK_INTERVAL_READS as usize + 2) {
            content.extend_from_slice(&vec![b'x'; READ_CHUNK_BYTES]);
            content.push(b'\n');
        }
        // The needle never appears, so `truncated` can never trigger the outer break first -
        // this isolates the cancellation-check path from the match-cap path.
        let cancellation = CancellationToken::new();
        cancellation.cancel();
        let result = search_content(
            content.as_slice(),
            &query("needle-not-present"),
            1_000_000,
            &cancellation,
        )
        .await;
        assert!(matches!(result, Err(VfsError::Cancelled)));
    }

    #[tokio::test]
    async fn a_pathologically_long_line_is_split_at_a_synthetic_boundary_without_oom() {
        let mut content = vec![b'y'; MAX_LINE_BYTES + 10];
        content.extend_from_slice(b"needle");
        content.push(b'\n');
        let outcome = search_content(
            content.as_slice(),
            &query("needle"),
            100,
            &CancellationToken::new(),
        )
        .await
        .expect("search must succeed");
        assert_eq!(outcome.matches.len(), 1);
        // The synthetic split does not advance line_number past the first real line.
        assert_eq!(outcome.matches[0].line_number, 1);
    }

    #[test]
    fn looks_like_binary_detects_a_nul_byte() {
        assert!(looks_like_binary(b"hello\0world"));
        assert!(!looks_like_binary(b"hello world"));
    }

    #[test]
    fn looks_like_binary_exempts_utf16_and_utf32_boms() {
        // "<?xml" encoded as UTF-16LE, as commonly produced by Windows tooling for .xml/.xsd.
        let utf16_le: Vec<u8> = "<?xml".encode_utf16().flat_map(u16::to_le_bytes).collect();
        let mut sample = UTF16_LE_BOM.to_vec();
        sample.extend(utf16_le);
        assert!(!looks_like_binary(&sample));

        let utf16_be: Vec<u8> = "<?xml".encode_utf16().flat_map(u16::to_be_bytes).collect();
        let mut sample = UTF16_BE_BOM.to_vec();
        sample.extend(utf16_be);
        assert!(!looks_like_binary(&sample));

        let mut sample = UTF32_LE_BOM.to_vec();
        sample.extend([b'<', 0, 0, 0]);
        assert!(!looks_like_binary(&sample));

        let mut sample = UTF32_BE_BOM.to_vec();
        sample.extend([0, 0, 0, b'<']);
        assert!(!looks_like_binary(&sample));
    }
}
