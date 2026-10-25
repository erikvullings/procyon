//! Splits an arbitrarily large JSON Lines stream into windows of whole lines.
//!
//! A host that cannot hold a multi-gigabyte export feeds the stream through a
//! [`JsonlWindower`] in whatever chunk size its transport delivers and
//! converts each emitted [`JsonlWindow`] independently, passing the window's
//! origin to the converter ([`DocumentMetadata::with_jsonl_origin`](crate::DocumentMetadata::with_jsonl_origin)).
//! Record provenance is then the absolute line and byte offset in the full
//! source, and concatenating the windows reproduces the source exactly, except
//! for lines too long to carry (see below).
//!
//! The windower is pure and synchronous: it owns at most one window plus one
//! partial line, so memory is bounded by the window limits and the longest
//! line the converter accepts, whatever the source size.
//!
//! A window never splits a line. A line longer than the converter reads is
//! replaced by the one-byte stand-in [`JSONL_OVERLONG_LINE_PLACEHOLDER`] in a
//! window of its own, so every later line keeps its true number and offset
//! while the converter reports the line as too long.

use crate::formats::jsonl::{JSONL_OVERLONG_LINE_PLACEHOLDER, MAX_LINE_BYTES};

/// Default byte limit of one window.
pub const DEFAULT_JSONL_WINDOW_BYTES: usize = 4 * 1024 * 1024;

/// Default line limit of one window. Half the default unit budget, so a
/// window never reaches the conversion unit limit.
pub const DEFAULT_JSONL_WINDOW_LINES: u32 = 10_000;

/// One window of whole lines and where it starts in the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonlWindow {
    /// 0-based position of the window in the source.
    pub index: u32,
    /// 1-based line number of the first line in the window.
    pub line: u64,
    /// 0-based byte offset of the window's first line in the source.
    pub byte_offset: u64,
    /// The window's lines, each including its terminator except possibly the
    /// source's final line.
    pub bytes: Vec<u8>,
}

/// Incremental splitter; see the module documentation.
#[derive(Debug)]
pub struct JsonlWindower {
    max_bytes: usize,
    max_lines: u32,
    max_line_bytes: usize,
    next_index: u32,
    next_line: u64,
    next_offset: u64,
    window: Vec<u8>,
    window_lines: u32,
    window_line: u64,
    window_offset: u64,
    tail: Vec<u8>,
    tail_length: u64,
    discarding: bool,
}

impl JsonlWindower {
    /// A windower closing windows at `max_bytes` or `max_lines`, whichever
    /// comes first. Both are raised to at least one.
    #[must_use]
    pub fn new(max_bytes: usize, max_lines: u32) -> Self {
        Self::with_line_limit(max_bytes, max_lines, MAX_LINE_BYTES)
    }

    fn with_line_limit(max_bytes: usize, max_lines: u32, max_line_bytes: usize) -> Self {
        Self {
            max_bytes: max_bytes.max(1),
            max_lines: max_lines.max(1),
            max_line_bytes,
            next_index: 0,
            next_line: 1,
            next_offset: 0,
            window: Vec::new(),
            window_lines: 0,
            window_line: 1,
            window_offset: 0,
            tail: Vec::new(),
            tail_length: 0,
            discarding: false,
        }
    }

    /// Feeds the next chunk of the source and returns the windows it closed.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<JsonlWindow> {
        let mut closed = Vec::new();
        let mut rest = chunk;
        while !rest.is_empty() {
            match rest.iter().position(|byte| *byte == b'\n') {
                Some(index) => {
                    self.take_terminated(&rest[..=index], &mut closed);
                    rest = &rest[index + 1..];
                }
                None => {
                    self.take_partial(rest, &mut closed);
                    rest = &[];
                }
            }
        }
        closed
    }

    /// Ends the source, returning the final window if any lines remain.
    pub fn finish(&mut self) -> Vec<JsonlWindow> {
        let mut closed = Vec::new();
        if self.tail_length > 0 {
            let tail = std::mem::take(&mut self.tail);
            if self.discarding || tail_is_overlong(self.tail_length, self.max_line_bytes) {
                self.discarding = false;
                self.close_window(&mut closed);
                let length = std::mem::take(&mut self.tail_length);
                self.emit_placeholder(length, &mut closed);
            } else {
                let length = std::mem::take(&mut self.tail_length);
                self.append_line(&tail, length, &mut closed);
            }
        }
        self.close_window(&mut closed);
        closed
    }

    fn take_partial(&mut self, part: &[u8], closed: &mut Vec<JsonlWindow>) {
        self.tail_length += part.len() as u64;
        if self.discarding {
            return;
        }
        if tail_is_overlong(self.tail_length, self.max_line_bytes) {
            // Stop buffering: the line is reported, not carried.
            self.discarding = true;
            self.tail = Vec::new();
            self.close_window(closed);
            return;
        }
        self.tail.extend_from_slice(part);
    }

    fn take_terminated(&mut self, part: &[u8], closed: &mut Vec<JsonlWindow>) {
        let length = self.tail_length + part.len() as u64;
        if self.discarding || tail_is_overlong(length - 1, self.max_line_bytes) {
            self.tail = Vec::new();
            self.tail_length = 0;
            self.discarding = false;
            self.close_window(closed);
            self.emit_placeholder(length, closed);
            return;
        }
        let mut line = std::mem::take(&mut self.tail);
        line.extend_from_slice(part);
        self.tail_length = 0;
        self.append_line(&line, length, closed);
    }

    fn append_line(&mut self, line: &[u8], length: u64, closed: &mut Vec<JsonlWindow>) {
        if self.window_lines == 0 {
            self.window_line = self.next_line;
            self.window_offset = self.next_offset;
        }
        self.window.extend_from_slice(line);
        self.window_lines += 1;
        self.next_line += 1;
        self.next_offset += length;
        if self.window.len() >= self.max_bytes || self.window_lines >= self.max_lines {
            self.close_window(closed);
        }
    }

    fn emit_placeholder(&mut self, length: u64, closed: &mut Vec<JsonlWindow>) {
        self.window_line = self.next_line;
        self.window_offset = self.next_offset;
        self.window
            .extend_from_slice(JSONL_OVERLONG_LINE_PLACEHOLDER);
        self.window.push(b'\n');
        self.window_lines = 1;
        self.next_line += 1;
        self.next_offset += length;
        self.close_window(closed);
    }

    fn close_window(&mut self, closed: &mut Vec<JsonlWindow>) {
        if self.window_lines == 0 {
            return;
        }
        closed.push(JsonlWindow {
            index: self.next_index,
            line: self.window_line,
            byte_offset: self.window_offset,
            bytes: std::mem::take(&mut self.window),
        });
        self.next_index += 1;
        self.window_lines = 0;
    }
}

fn tail_is_overlong(length: u64, max_line_bytes: usize) -> bool {
    length > max_line_bytes as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        BaselineConverter, ConversionContext, DocumentConverter, DocumentMetadata, Provenance,
        SourceContent,
    };

    fn windows_of(source: &[u8], chunk: usize, bytes: usize, lines: u32) -> Vec<JsonlWindow> {
        windows_with(JsonlWindower::new(bytes, lines), source, chunk)
    }

    fn windows_with(mut windower: JsonlWindower, source: &[u8], chunk: usize) -> Vec<JsonlWindow> {
        let mut windows = Vec::new();
        for part in source.chunks(chunk.max(1)) {
            windows.extend(windower.push(part));
        }
        windows.extend(windower.finish());
        windows
    }

    fn joined(windows: &[JsonlWindow]) -> Vec<u8> {
        windows
            .iter()
            .flat_map(|window| window.bytes.iter().copied())
            .collect()
    }

    #[test]
    fn windows_hold_whole_lines_and_concatenate_to_the_source_for_any_chunking() {
        let source = b"{\"a\":1}\r\n\r\n{\"b\":22}\n{\"c\":333}\n\n{\"d\":4}";
        for chunk in [1, 2, 3, 5, 7, 1_000] {
            for bytes in [1, 10, 20, 1_000] {
                let windows = windows_of(source, chunk, bytes, 1_000);
                assert_eq!(joined(&windows), source, "chunk {chunk} bytes {bytes}");
                let mut line = 1_u64;
                let mut offset = 0_u64;
                for (index, window) in windows.iter().enumerate() {
                    assert_eq!(window.index as usize, index);
                    assert_eq!((window.line, window.byte_offset), (line, offset));
                    line += window.bytes.iter().filter(|byte| **byte == b'\n').count() as u64
                        + u64::from(!window.bytes.ends_with(b"\n"));
                    offset += window.bytes.len() as u64;
                }
            }
        }
    }

    #[test]
    fn the_line_limit_closes_windows_and_blank_lines_still_count() {
        let windows = windows_of(b"a\n\nb\n\nc\n", 3, 1_000, 2);
        let shape = windows
            .iter()
            .map(|window| {
                (
                    window.index,
                    window.line,
                    window.byte_offset,
                    window.bytes.len(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(shape, [(0, 1, 0, 3), (1, 3, 3, 3), (2, 5, 6, 2)]);
    }

    #[test]
    fn an_empty_source_has_no_windows() {
        assert!(windows_of(b"", 4, 10, 10).is_empty());
    }

    #[test]
    fn an_overlong_line_becomes_a_placeholder_window_that_keeps_later_positions_exact() {
        let long = "x".repeat(40);
        let source = format!("{{\"text\":\"a\"}}\n{long}\n{{\"text\":\"b\"}}\n");
        for chunk in [1, 7, 1_000] {
            let windows = windows_with(
                JsonlWindower::with_line_limit(1_000, 100, 16),
                source.as_bytes(),
                chunk,
            );
            let shape = windows
                .iter()
                .map(|window| (window.line, window.byte_offset, window.bytes.clone()))
                .collect::<Vec<_>>();
            assert_eq!(
                shape,
                vec![
                    (1, 0, b"{\"text\":\"a\"}\n".to_vec()),
                    (2, 13, b"\x1e\n".to_vec()),
                    (3, 13 + 41, b"{\"text\":\"b\"}\n".to_vec()),
                ],
                "chunk {chunk}"
            );
        }
        // An unterminated overlong final line is also replaced.
        let windows = windows_with(
            JsonlWindower::with_line_limit(1_000, 100, 16),
            long.as_bytes(),
            5,
        );
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].bytes, b"\x1e\n");
    }

    #[test]
    fn converting_windows_separately_matches_converting_the_whole_source() {
        let source = (1..=60)
            .map(|index| match index % 7 {
                0 => "not json\n".to_owned(),
                3 => "\n".to_owned(),
                _ => format!("{{\"title\":\"T{index}\",\"text\":\"record number {index}\"}}\n"),
            })
            .collect::<String>();
        let positions = |bytes: &[u8], origin: Option<(u64, u64)>| {
            let mut metadata = DocumentMetadata::unknown().with_extension("jsonl");
            if let Some((line, offset)) = origin {
                metadata = metadata.with_jsonl_origin(line, offset);
            }
            BaselineConverter::new()
                .convert(
                    SourceContent::Bytes(bytes),
                    &metadata,
                    &ConversionContext::new(),
                )
                .expect("conversion")
                .document()
                .map(|document| {
                    document
                        .units()
                        .iter()
                        .map(|unit| match unit.provenance {
                            Provenance::JsonlRecord { line, byte_offset } => {
                                (line, byte_offset, unit.text.clone())
                            }
                            ref other => panic!("unexpected {other:?}"),
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        };
        let whole = positions(source.as_bytes(), None);
        for (bytes, lines) in [(64, 1_000), (1_000_000, 4), (200, 5)] {
            let windows = windows_of(source.as_bytes(), 13, bytes, lines);
            assert!(windows.len() > 1);
            let split = windows
                .iter()
                .flat_map(|window| {
                    positions(&window.bytes, Some((window.line, window.byte_offset)))
                })
                .collect::<Vec<_>>();
            assert_eq!(split, whole, "bytes {bytes} lines {lines}");
        }
    }
}
