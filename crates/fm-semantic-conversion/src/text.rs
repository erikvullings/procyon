//! Decoding, sanitization, source-position accounting and the conservative
//! instruction-shaped-text detector.
//!
//! # What sanitization removes, and what it deliberately does not
//!
//! Only *invisible* characters that cannot be rendered but can change how text
//! is read (or hide instructions from a reviewer) are removed:
//!
//! * `U+0000` and the C0 control characters other than tab and newline,
//! * the bidirectional overrides and embeddings `U+202A..=U+202E`,
//! * the bidirectional isolates `U+2066..=U+2069`,
//! * the invisible marks `U+200B` (zero-width space), `U+200E`/`U+200F`
//!   (left/right-to-left mark), `U+2060` (word joiner), `U+FEFF`
//!   (zero-width no-break space / stray BOM),
//! * the Unicode tag characters `U+E0000..=U+E007F`, which carry no visible
//!   glyph and are the standard vehicle for hidden prompt text.
//!
//! `U+200C`/`U+200D` (zero-width non-joiner/joiner) are **kept**: they are
//! semantically required by Persian, Indic scripts and emoji sequences, and
//! removing them corrupts legitimate text.
//!
//! Visible text is never rewritten. Text that reads like an instruction to a
//! model is retained verbatim and merely flagged - see
//! [`instruction_like_excerpt`] - because rewriting it would destroy the
//! evidence a reviewer needs, and a sanitizer is not a security boundary.

use encoding_rs::{Encoding, UTF_8, WINDOWS_1252};

/// Result of decoding source bytes to text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DecodedText {
    /// Decoded text, with any byte-order mark removed.
    pub text: String,
    /// Name of the encoding that was used.
    pub encoding: &'static str,
    /// Whether decoding had to substitute replacement characters.
    pub lossy: bool,
    /// Number of leading bytes consumed by a byte-order mark.
    pub bom_bytes: usize,
}

/// Decodes `bytes` into text.
///
/// Resolution order is deterministic: a byte-order mark wins (UTF-8, UTF-16LE
/// and UTF-16BE are recognized), then an explicitly declared charset label,
/// then strict UTF-8, and finally Windows-1252 - the standard fallback for
/// legacy single-byte text, which never fails and is marked as a lossy decode
/// only when the bytes were not already valid in that encoding.
#[must_use]
pub fn decode(bytes: &[u8], declared_charset: Option<&str>) -> DecodedText {
    if let Some((encoding, bom_length)) = Encoding::for_bom(bytes) {
        let (text, _, had_errors) = encoding.decode(&bytes[bom_length..]);
        return DecodedText {
            text: text.into_owned(),
            encoding: encoding.name(),
            lossy: had_errors,
            bom_bytes: bom_length,
        };
    }
    if let Some(encoding) = declared_charset.and_then(|label| Encoding::for_label(label.as_bytes()))
    {
        let (text, had_errors) = encoding.decode_without_bom_handling(bytes);
        return DecodedText {
            text: text.into_owned(),
            encoding: encoding.name(),
            lossy: had_errors,
            bom_bytes: 0,
        };
    }
    let (text, had_errors) = UTF_8.decode_without_bom_handling(bytes);
    if !had_errors {
        return DecodedText {
            text: text.into_owned(),
            encoding: UTF_8.name(),
            lossy: false,
            bom_bytes: 0,
        };
    }
    let (text, _, had_errors) = WINDOWS_1252.decode(bytes);
    DecodedText {
        text: text.into_owned(),
        encoding: WINDOWS_1252.name(),
        lossy: had_errors,
        bom_bytes: 0,
    }
}

/// A contiguous stretch of sanitized output that maps linearly onto the
/// source.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceSegment {
    /// First character offset of the segment in the sanitized text.
    pub output_start: usize,
    /// Character offset in the decoded source the segment starts at.
    pub source_start: usize,
    /// Length of the segment in characters.
    pub length: usize,
}

/// Maps sanitized character offsets back onto decoded source offsets.
///
/// Sanitization only ever *deletes* characters, so the mapping is a list of
/// linear segments rather than a per-character table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SourceMap {
    segments: Vec<SourceSegment>,
}

impl SourceMap {
    /// The segments in output order.
    #[must_use]
    pub fn segments(&self) -> &[SourceSegment] {
        &self.segments
    }

    /// Source character offset corresponding to `output_offset`, if that
    /// offset falls inside the mapped text.
    #[must_use]
    pub fn source_offset(&self, output_offset: usize) -> Option<usize> {
        self.segments.iter().find_map(|segment| {
            let end = segment.output_start + segment.length;
            (output_offset >= segment.output_start && output_offset < end)
                .then(|| segment.source_start + (output_offset - segment.output_start))
        })
    }

    /// Source character offset of the first mapped character.
    #[must_use]
    pub fn first_source_offset(&self) -> Option<usize> {
        self.segments.first().map(|segment| segment.source_start)
    }

    /// Source character offset one past the last mapped character.
    #[must_use]
    pub fn end_source_offset(&self) -> Option<usize> {
        self.segments
            .last()
            .map(|segment| segment.source_start + segment.length)
    }

    /// Restricts the map to the sanitized character range `start..end`,
    /// rebasing output offsets onto the new slice. Used when an oversized
    /// unit is truncated or split.
    #[must_use]
    pub fn slice(&self, start: usize, end: usize) -> Self {
        let mut segments = Vec::new();
        for segment in &self.segments {
            let segment_end = segment.output_start + segment.length;
            let overlap_start = segment.output_start.max(start);
            let overlap_end = segment_end.min(end);
            if overlap_start >= overlap_end {
                continue;
            }
            segments.push(SourceSegment {
                output_start: overlap_start - start,
                source_start: segment.source_start + (overlap_start - segment.output_start),
                length: overlap_end - overlap_start,
            });
        }
        Self { segments }
    }
}

/// Sanitized text plus the accounting needed to point back at the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SanitizedText {
    /// The sanitized text.
    pub text: String,
    /// Mapping from sanitized offsets back to decoded-source offsets.
    pub map: SourceMap,
    /// How many invisible or control characters were removed.
    pub removed: u32,
}

/// Whether `character` is one of the invisible or control hazards listed in
/// the module documentation.
fn is_hazard(character: char) -> bool {
    match character {
        '\t' | '\n' => false,
        '\u{200c}' | '\u{200d}' => false,
        control if control.is_control() => true,
        '\u{200b}' | '\u{200e}' | '\u{200f}' | '\u{2060}' | '\u{feff}' => true,
        '\u{202a}'..='\u{202e}' => true,
        '\u{2066}'..='\u{2069}' => true,
        '\u{e0000}'..='\u{e007f}' => true,
        _ => false,
    }
}

/// Removes invisible and control hazards from `input`, keeping an origin map.
///
/// `source_offset` is the character offset of `input` inside the decoded
/// source document, so units extracted from the middle of a file still report
/// absolute source positions. Carriage returns are dropped (`\r\n` becomes
/// `\n`) and counted as removals like any other control character.
#[must_use]
pub fn sanitize(input: &str, source_offset: usize) -> SanitizedText {
    let mut text = String::with_capacity(input.len());
    let mut segments: Vec<SourceSegment> = Vec::new();
    let mut removed = 0_u32;
    let mut output_chars = 0_usize;
    for (index, character) in input.chars().enumerate() {
        if is_hazard(character) {
            removed = removed.saturating_add(1);
            continue;
        }
        let source_position = source_offset + index;
        match segments.last_mut() {
            Some(segment)
                if segment.source_start + segment.length == source_position
                    && segment.output_start + segment.length == output_chars =>
            {
                segment.length += 1;
            }
            _ => segments.push(SourceSegment {
                output_start: output_chars,
                source_start: source_position,
                length: 1,
            }),
        }
        text.push(character);
        output_chars += 1;
    }
    SanitizedText {
        text,
        map: SourceMap { segments },
        removed,
    }
}

/// Phrases that mark text as instruction-shaped. Kept deliberately short and
/// specific: a false positive costs a reviewer's attention, and the detector
/// never changes the text it flags.
const INSTRUCTION_PHRASES: &[&str] = &[
    "ignore previous instructions",
    "ignore all previous instructions",
    "ignore the above instructions",
    "disregard previous instructions",
    "disregard the above",
    "system prompt",
    "you are an ai",
    "as an ai language model",
    "act as an ai",
    "new instructions:",
    "override your instructions",
];

/// Returns a short excerpt when `text` contains instruction-shaped content.
///
/// The match is case-insensitive and whitespace-normalized so that line
/// wrapping does not hide a phrase. The caller records a warning; it must not
/// alter the text.
#[must_use]
pub fn instruction_like_excerpt(text: &str) -> Option<String> {
    let normalized: String = {
        let mut buffer = String::with_capacity(text.len());
        let mut pending_space = false;
        for character in text.chars() {
            if character.is_whitespace() {
                pending_space = !buffer.is_empty();
                continue;
            }
            if pending_space {
                buffer.push(' ');
                pending_space = false;
            }
            buffer.extend(character.to_lowercase());
        }
        buffer
    };
    let position = INSTRUCTION_PHRASES
        .iter()
        .filter_map(|phrase| normalized.find(phrase))
        .min()?;
    let excerpt: String = normalized[position..].chars().take(120).collect();
    Some(excerpt)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_utf8_bom_is_stripped_and_reported() {
        let decoded = decode("\u{feff}hello".as_bytes(), None);
        assert_eq!(decoded.text, "hello");
        assert_eq!(decoded.encoding, "UTF-8");
        assert_eq!(decoded.bom_bytes, 3);
        assert!(!decoded.lossy);
    }

    #[test]
    fn utf16_byte_order_marks_are_recognized_in_both_endiannesses() {
        let mut little = vec![0xFF, 0xFE];
        little.extend("hi".encode_utf16().flat_map(u16::to_le_bytes));
        let mut big = vec![0xFE, 0xFF];
        big.extend("hi".encode_utf16().flat_map(u16::to_be_bytes));
        assert_eq!(decode(&little, None).text, "hi");
        assert_eq!(decode(&little, None).encoding, "UTF-16LE");
        assert_eq!(decode(&big, None).text, "hi");
        assert_eq!(decode(&big, None).encoding, "UTF-16BE");
    }

    #[test]
    fn a_declared_charset_decodes_legacy_bytes() {
        let decoded = decode(&[0xE9, 0x74, 0xE9], Some("iso-8859-1"));
        assert_eq!(decoded.text, "été");
        assert_eq!(decoded.encoding, "windows-1252");
        assert!(!decoded.lossy);
    }

    #[test]
    fn invalid_utf8_falls_back_to_windows_1252() {
        let decoded = decode(&[0xE9, 0x74, 0xE9], None);
        assert_eq!(decoded.text, "été");
        assert_eq!(decoded.encoding, "windows-1252");
    }

    #[test]
    fn sanitization_removes_hazards_and_keeps_source_positions() {
        let input = "a\u{202e}b\u{0000}c\r\nd";
        let sanitized = sanitize(input, 100);
        assert_eq!(sanitized.text, "abc\nd");
        assert_eq!(sanitized.removed, 3);
        assert_eq!(sanitized.map.source_offset(0), Some(100));
        assert_eq!(sanitized.map.source_offset(1), Some(102));
        assert_eq!(sanitized.map.source_offset(2), Some(104));
        assert_eq!(sanitized.map.source_offset(4), Some(107));
    }

    #[test]
    fn zero_width_joiners_are_preserved() {
        let sanitized = sanitize("a\u{200d}b\u{200c}c", 0);
        assert_eq!(sanitized.text, "a\u{200d}b\u{200c}c");
        assert_eq!(sanitized.removed, 0);
    }

    #[test]
    fn unicode_tag_characters_are_removed() {
        let sanitized = sanitize("visible\u{e0041}\u{e0042}", 0);
        assert_eq!(sanitized.text, "visible");
        assert_eq!(sanitized.removed, 2);
    }

    #[test]
    fn slicing_a_source_map_rebases_offsets() {
        let sanitized = sanitize("a\u{202e}bcdef", 10);
        let sliced = sanitized.map.slice(2, 4);
        assert_eq!(sliced.source_offset(0), Some(13));
        assert_eq!(sliced.source_offset(1), Some(14));
        assert_eq!(sliced.source_offset(2), None);
    }

    #[test]
    fn instruction_shaped_text_is_detected_across_line_breaks_and_case() {
        let excerpt = instruction_like_excerpt("Please\nIGNORE PREVIOUS\nINSTRUCTIONS and reply")
            .expect("flagged");
        assert!(excerpt.starts_with("ignore previous instructions"));
        assert!(instruction_like_excerpt("An ordinary paragraph about budgets.").is_none());
    }
}
