//! Baseline format converters.
//!
//! Each submodule turns one family of bytes into [`StructuralUnit`]s through
//! the shared [`DocumentBuilder`](crate::builder::DocumentBuilder), so
//! sanitization, budgets and omission bookkeeping are identical across
//! formats.

pub(crate) mod csv;
pub(crate) mod docx;
pub(crate) mod epub;
pub(crate) mod html;
pub(crate) mod markdown;
pub(crate) mod package;
pub(crate) mod pdf;
pub(crate) mod plain;
pub(crate) mod pptx;
pub(crate) mod spreadsheet;

/// Byte-offset to line index over one decoded source text.
pub(crate) struct LineIndex {
    /// Byte offset of the start of each line.
    starts: Vec<usize>,
}

impl LineIndex {
    pub(crate) fn new(text: &str) -> Self {
        let mut starts = vec![0];
        for (byte, character) in text.char_indices() {
            if character == '\n' {
                starts.push(byte + character.len_utf8());
            }
        }
        Self { starts }
    }

    /// 1-based line containing `byte`.
    pub(crate) fn line_of(&self, byte: usize) -> u32 {
        let index = match self.starts.binary_search(&byte) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        (index + 1) as u32
    }
}

/// A run of consecutive non-blank lines, described as a range of the source
/// so that sanitization can account for every removed character.
pub(crate) struct TextBlock {
    pub(crate) start_line: u32,
    pub(crate) end_line: u32,
    pub(crate) char_offset: usize,
    pub(crate) byte_start: usize,
    pub(crate) byte_end: usize,
}

impl TextBlock {
    /// The block's raw source text, still containing any carriage returns
    /// that sanitization will remove and account for.
    pub(crate) fn text<'a>(&self, source: &'a str) -> &'a str {
        &source[self.byte_start..self.byte_end]
    }
}

/// Splits `text` into blank-line separated blocks, tracking 1-based line
/// numbers and character offsets so provenance and source maps stay exact.
pub(crate) fn split_blocks(text: &str) -> Vec<TextBlock> {
    let mut blocks = Vec::new();
    let mut current: Option<(u32, usize, usize)> = None;
    let mut last_content_line = 0_u32;
    let mut last_content_end = 0_usize;
    let mut line_number = 0_u32;
    let mut char_offset = 0_usize;
    let mut byte_offset = 0_usize;
    for line in text.split_inclusive('\n') {
        line_number += 1;
        let content = line.trim_end_matches(['\n', '\r']);
        if content.trim().is_empty() {
            if let Some((start_line, offset, byte_start)) = current.take() {
                blocks.push(TextBlock {
                    start_line,
                    end_line: last_content_line,
                    char_offset: offset,
                    byte_start,
                    byte_end: last_content_end,
                });
            }
        } else {
            if current.is_none() {
                current = Some((line_number, char_offset, byte_offset));
            }
            last_content_line = line_number;
            last_content_end = byte_offset + content.len();
        }
        char_offset += line.chars().count();
        byte_offset += line.len();
    }
    if let Some((start_line, offset, byte_start)) = current {
        blocks.push(TextBlock {
            start_line,
            end_line: last_content_line,
            char_offset: offset,
            byte_start,
            byte_end: last_content_end,
        });
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_line_index_maps_bytes_to_lines() {
        let text = "héllo\nworld\n\nlast";
        let index = LineIndex::new(text);
        assert_eq!(index.line_of(0), 1);
        assert_eq!(index.line_of(text.find("world").expect("world")), 2);
        assert_eq!(index.line_of(text.find("last").expect("last")), 4);
    }

    #[test]
    fn blocks_are_split_on_blank_lines_with_exact_positions() {
        let text = "first line\nstill first\n\n\nsecond block\n";
        let blocks = split_blocks(text);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text(text), "first line\nstill first");
        assert_eq!((blocks[0].start_line, blocks[0].end_line), (1, 2));
        assert_eq!(blocks[0].char_offset, 0);
        assert_eq!(blocks[1].text(text), "second block");
        assert_eq!((blocks[1].start_line, blocks[1].end_line), (5, 5));
        assert_eq!(
            blocks[1].char_offset,
            text.find("second").expect("second block")
        );
    }

    #[test]
    fn carriage_returns_stay_in_the_block_slice_for_sanitization_to_account_for() {
        let source = "alpha\r\n\r\nbeta\r\n";
        let blocks = split_blocks(source);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].text(source), "alpha");
        assert_eq!(blocks[1].text(source), "beta");
        assert_eq!((blocks[1].start_line, blocks[1].end_line), (3, 3));
    }
}
