//! JSON Lines (NDJSON) conversion.
//!
//! A JSON Lines export (a channel or article feed, a log of events) can be
//! arbitrarily large, so unlike every other baseline format it is *streamed*:
//! the source is read through a bounded window, one whole line at a time, and
//! never held in memory. Memory is bounded by the window plus
//! [`MAX_LINE_BYTES`], independent of the file size, and the converter stops
//! reading as soon as an output budget is saturated.
//!
//! Each line is one record. A record becomes one structural unit whose body is
//! its summary and text; its title, channel and publication date go into the
//! unit's section path exactly once, never into the body, so the chunker adds
//! them as context rather than every chunk repeating them in its text. Opaque
//! values (data URIs, long base64 strings such as inline images) are omitted
//! and counted. Lines that cannot be used are skipped and reported as typed
//! warnings carrying their 1-based line number; they never fail the document.
//!
//! Provenance is the record's 1-based line number and byte offset. Neither is
//! derived from a name or path, so chunk fingerprints stay path-free.

use std::collections::HashMap;
use std::io::{self, Read};

use serde_json::{Map, Value};

use crate::budget::{BudgetKind, Stop};
use crate::builder::{DocumentBuilder, UnitDraft};
use crate::model::{
    ConversionWarning, JsonlOrigin, Omission, Provenance, SkippedRecordReason, TopLevelBoundary,
    UnitKind,
};
use crate::text::sanitize;

/// Bytes requested from the source per read.
pub(crate) const WINDOW_BYTES: usize = 1024 * 1024;

/// Longest line that is parsed. A longer line is drained from the stream and
/// reported, so one pathological record cannot exhaust memory.
pub(crate) const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;

/// Content of the stand-in line that a windower writes for a source line that
/// was too long to carry in any window. It keeps line numbers and byte offsets
/// of everything after it exact, and is reported as a too-long line.
pub const JSONL_OVERLONG_LINE_PLACEHOLDER: &[u8] = b"\x1e";

use JSONL_OVERLONG_LINE_PLACEHOLDER as OVERLONG_LINE_PLACEHOLDER;

/// Skipped records itemized individually; the total is always reported.
const MAX_ITEMIZED_SKIPS: usize = 32;

/// Longest value kept for a field that is not a known record field.
const MAX_FALLBACK_VALUE_CHARS: usize = 256;

/// Most unknown fields kept for one record.
const MAX_FALLBACK_FIELDS: usize = 16;

/// Distinct unknown field names remembered for constant suppression.
const MAX_REMEMBERED_FIELDS: usize = 256;

/// Longest title, channel or date kept in a section path element.
const MAX_METADATA_CHARS: usize = 200;

/// Strings at least this long that look like base64 are opaque blobs.
const BLOB_MIN_CHARS: usize = 256;

/// Bytes inspected for NUL when deciding that a source is binary.
const BINARY_SNIFF_BYTES: usize = 8 * 1024;

const TITLE_KEYS: &[&str] = &["title", "headline", "subject", "name"];
const SUMMARY_KEYS: &[&str] = &["summary", "abstract", "description", "excerpt"];
const TEXT_KEYS: &[&str] = &["text", "content", "body", "message", "article"];
const DATE_KEYS: &[&str] = &[
    "date",
    "published",
    "pubdate",
    "publishedat",
    "datepublished",
    "publicationdate",
    "createdat",
    "timestamp",
];
const CHANNEL_KEYS: &[&str] = &["channel", "channelname", "source", "feed", "site"];

/// Why a JSON Lines conversion stopped without a document.
#[derive(Debug)]
pub(crate) enum JsonlError {
    /// The source could not be read.
    Read(io::Error),
    /// Cancellation or a hard budget stopped the conversion.
    Stopped(Stop),
    /// The leading bytes contain NUL, so this is not text.
    Binary,
}

impl From<Stop> for JsonlError {
    fn from(stop: Stop) -> Self {
        Self::Stopped(stop)
    }
}

impl From<io::Error> for JsonlError {
    fn from(error: io::Error) -> Self {
        Self::Read(error)
    }
}

/// What a conversion saw, beyond the units it emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct JsonlStats {
    /// Bytes consumed from the source.
    pub(crate) bytes_read: u64,
    /// Lines that produced a record.
    pub(crate) records: u64,
    /// Non-blank lines that were skipped.
    pub(crate) skipped: u64,
    /// First skipped line, for diagnostics.
    pub(crate) first_skipped_line: Option<u64>,
}

/// Position and shape of one physical line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RawLine {
    /// 1-based line number.
    pub(crate) number: u64,
    /// 0-based byte offset of the line's first byte.
    pub(crate) offset: u64,
    /// Whether the line exceeded the maximum length. Its content is dropped.
    pub(crate) overlong: bool,
}

/// Reads whole lines from a stream through a fixed window.
///
/// A line that straddles a window edge is reassembled; the window only bounds
/// how much is requested from the source at a time. The running line number
/// and byte offset are carried across windows, so positions are always
/// absolute.
pub(crate) struct LineReader<'r> {
    reader: &'r mut dyn Read,
    window: Box<[u8]>,
    start: usize,
    end: usize,
    exhausted: bool,
    upcoming_line: u64,
    next_offset: u64,
    origin_offset: u64,
    max_line_bytes: usize,
}

impl<'r> LineReader<'r> {
    pub(crate) fn new(
        reader: &'r mut dyn Read,
        window_bytes: usize,
        max_line_bytes: usize,
        origin: JsonlOrigin,
    ) -> Self {
        Self {
            reader,
            window: vec![0; window_bytes.max(1)].into_boxed_slice(),
            start: 0,
            end: 0,
            exhausted: false,
            upcoming_line: origin.line,
            next_offset: origin.byte_offset,
            origin_offset: origin.byte_offset,
            max_line_bytes,
        }
    }

    /// 1-based number of the line that will be read next.
    pub(crate) fn upcoming_line(&self) -> u64 {
        self.upcoming_line
    }

    /// Bytes consumed so far.
    pub(crate) fn bytes_read(&self) -> u64 {
        self.next_offset - self.origin_offset
    }

    fn refill(&mut self) -> io::Result<()> {
        loop {
            match self.reader.read(&mut self.window) {
                Ok(0) => {
                    self.exhausted = true;
                    self.start = 0;
                    self.end = 0;
                    return Ok(());
                }
                Ok(read) => {
                    self.start = 0;
                    self.end = read;
                    return Ok(());
                }
                Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
                Err(error) => return Err(error),
            }
        }
    }

    /// Reads the next line into `buffer`, without its terminator or a
    /// preceding carriage return. Returns `None` at the end of the stream.
    pub(crate) fn next_line(&mut self, buffer: &mut Vec<u8>) -> io::Result<Option<RawLine>> {
        buffer.clear();
        let mut total = 0_u64;
        let mut overlong = false;
        loop {
            if self.start == self.end {
                if self.exhausted {
                    break;
                }
                self.refill()?;
                continue;
            }
            let available = &self.window[self.start..self.end];
            let (content, consumed, terminated) =
                match available.iter().position(|byte| *byte == b'\n') {
                    Some(index) => (&available[..index], index + 1, true),
                    None => (available, available.len(), false),
                };
            if !overlong {
                if buffer.len() + content.len() > self.max_line_bytes {
                    overlong = true;
                    buffer.clear();
                } else {
                    buffer.extend_from_slice(content);
                }
            }
            self.start += consumed;
            total += consumed as u64;
            if terminated {
                break;
            }
        }
        if total == 0 {
            return Ok(None);
        }
        if buffer.last() == Some(&b'\r') {
            buffer.pop();
        }
        let line = RawLine {
            number: self.upcoming_line,
            offset: self.next_offset,
            overlong,
        };
        self.upcoming_line += 1;
        self.next_offset += total;
        Ok(Some(line))
    }
}

/// Unit body and metadata extracted from one record.
#[derive(Debug, Default, PartialEq, Eq)]
struct Record {
    title: Option<String>,
    channel: Option<String>,
    date: Option<String>,
    body: Vec<String>,
    omitted_blobs: u64,
}

impl Record {
    fn into_draft(self, line: u64, byte_offset: u64) -> Option<UnitDraft> {
        let Self {
            title,
            channel,
            date,
            mut body,
            ..
        } = self;
        let mut title = title;
        if body.is_empty() {
            // A record that is only a title is still worth finding.
            body.push(title.take()?);
        }
        let metadata = [channel, date]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>()
            .join(" \u{b7} ");
        let section_path = title
            .into_iter()
            .chain((!metadata.is_empty()).then_some(metadata))
            .collect();
        Some(UnitDraft {
            kind: UnitKind::Paragraph,
            section_path,
            text: body.join("\n\n"),
            provenance: Provenance::JsonlRecord { line, byte_offset },
            boundary: TopLevelBoundary::Section(format!("record {line}")),
            source_offset: None,
        })
    }
}

/// Lowercases a field name and drops separators, so `published_at`,
/// `publishedAt` and `Published-At` are one key.
fn normalize_key(key: &str) -> String {
    key.chars()
        .filter(|character| !matches!(character, '_' | '-' | ' '))
        .flat_map(char::to_lowercase)
        .collect()
}

/// Whether `value` is an opaque binary payload rather than prose.
fn is_opaque_blob(value: &str) -> bool {
    let trimmed = value.trim_start();
    if trimmed.len() >= 5 && trimmed[..5].eq_ignore_ascii_case("data:") {
        return true;
    }
    if value.chars().count() < BLOB_MIN_CHARS {
        return false;
    }
    let mut total = 0_usize;
    let mut base64 = 0_usize;
    for character in value.chars() {
        total += 1;
        if character.is_ascii_alphanumeric() || matches!(character, '+' | '/' | '=' | '-' | '_') {
            base64 += 1;
        } else if character.is_whitespace() {
            // Prose has spaces; base64 does not (except MIME line breaks).
            if character == ' ' {
                return false;
            }
            total -= 1;
        }
    }
    total > 0 && base64 * 100 >= total * 97
}

/// Civil date for a day count since 1970-01-01 (proleptic Gregorian).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let shifted = days + 719_468;
    let era = shifted.div_euclid(146_097);
    let day_of_era = shifted.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * month_index + 2) / 5 + 1) as u32;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    } as u32;
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

/// Renders a Unix timestamp (seconds or milliseconds) as `YYYY-MM-DD`.
fn epoch_date(value: i64) -> Option<String> {
    let seconds = if value.unsigned_abs() >= 100_000_000_000 {
        value / 1_000
    } else {
        value
    };
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    (0..=9_999)
        .contains(&year)
        .then(|| format!("{year:04}-{month:02}-{day:02}"))
}

/// Scalar field value as text.
fn scalar(value: &Value, as_date: bool) -> Option<String> {
    match value {
        Value::String(text) => Some(text.clone()),
        Value::Number(number) if as_date => number.as_i64().and_then(epoch_date),
        Value::Number(number) => Some(number.to_string()),
        _ => None,
    }
}

fn clean(text: &str, limit: usize) -> Option<String> {
    let sanitized = sanitize(text, 0).text;
    let collapsed = sanitized.split_whitespace().collect::<Vec<_>>().join(" ");
    let value: String = collapsed.chars().take(limit).collect();
    (!value.is_empty()).then_some(value)
}

/// One record field resolved against its alias list.
fn lookup<'a>(entries: &'a [(String, &'a Value)], keys: &[&str]) -> Option<&'a Value> {
    keys.iter().find_map(|key| {
        entries
            .iter()
            .find(|(candidate, value)| candidate == key && !value.is_null())
            .map(|(_, value)| *value)
    })
}

/// Remembers the last emitted value of each unknown field so a value that
/// every record repeats is emitted once instead of in every record.
#[derive(Default)]
struct ConstantFilter {
    last: HashMap<String, String>,
}

impl ConstantFilter {
    /// Whether `value` differs from the previous record's value for `key`.
    fn is_new(&mut self, key: &str, value: &str) -> bool {
        if let Some(previous) = self.last.get_mut(key) {
            if previous == value {
                return false;
            }
            value.clone_into(previous);
            return true;
        }
        if self.last.len() < MAX_REMEMBERED_FIELDS {
            self.last.insert(key.to_owned(), value.to_owned());
        }
        true
    }
}

fn extract_record(object: &Map<String, Value>, constants: &mut ConstantFilter) -> Record {
    let entries = object
        .iter()
        .map(|(key, value)| (normalize_key(key), value))
        .collect::<Vec<_>>();
    let mut record = Record::default();

    let text_value = |keys: &[&str], record: &mut Record| -> Option<String> {
        let value = lookup(&entries, keys)?;
        let raw = match value {
            Value::String(text) => Some(text.clone()),
            Value::Array(items) => {
                let parts = items.iter().filter_map(Value::as_str).collect::<Vec<_>>();
                (!parts.is_empty()).then(|| parts.join("\n"))
            }
            _ => None,
        }?;
        if is_opaque_blob(&raw) {
            record.omitted_blobs += 1;
            return None;
        }
        let sanitized = sanitize(&raw, 0).text;
        let trimmed = sanitized.trim();
        (!trimmed.is_empty()).then(|| trimmed.to_owned())
    };
    let metadata = |keys: &[&str], as_date: bool, record: &mut Record| -> Option<String> {
        let value = lookup(&entries, keys)?;
        let raw = scalar(value, as_date)?;
        if is_opaque_blob(&raw) {
            record.omitted_blobs += 1;
            return None;
        }
        clean(&raw, MAX_METADATA_CHARS)
    };

    record.title = metadata(TITLE_KEYS, false, &mut record);
    record.channel = metadata(CHANNEL_KEYS, false, &mut record);
    record.date = metadata(DATE_KEYS, true, &mut record);
    let summary = text_value(SUMMARY_KEYS, &mut record);
    let text = text_value(TEXT_KEYS, &mut record);
    let known_text = summary.is_some() || text.is_some();
    record.body.extend(summary);
    record.body.extend(text);

    if record.title.is_none() && !known_text {
        let known = [TITLE_KEYS, SUMMARY_KEYS, TEXT_KEYS, DATE_KEYS, CHANNEL_KEYS].concat();
        let mut lines = Vec::new();
        for (key, value) in object {
            if known.contains(&normalize_key(key).as_str()) {
                continue;
            }
            let Some(raw) = value.as_str() else { continue };
            if is_opaque_blob(raw) {
                record.omitted_blobs += 1;
                continue;
            }
            let Some(value) = clean(raw, MAX_FALLBACK_VALUE_CHARS) else {
                continue;
            };
            if raw.chars().count() > MAX_FALLBACK_VALUE_CHARS
                || lines.len() >= MAX_FALLBACK_FIELDS
                || !constants.is_new(key, &value)
            {
                continue;
            }
            lines.push(format!("{key}: {value}"));
        }
        if !lines.is_empty() {
            record.body.push(lines.join("\n"));
        }
    }
    record
}

/// Parses one line. `Ok(None)` is a blank line.
fn parse_line(
    content: &[u8],
    constants: &mut ConstantFilter,
) -> Result<Option<Record>, SkippedRecordReason> {
    let Ok(text) = std::str::from_utf8(content) else {
        return Err(SkippedRecordReason::InvalidUtf8);
    };
    if text.trim().is_empty() {
        return Ok(None);
    }
    match serde_json::from_str::<Value>(text) {
        Ok(Value::Object(object)) => Ok(Some(extract_record(&object, constants))),
        Ok(_) => Err(SkippedRecordReason::NotAnObject),
        Err(_) => Err(SkippedRecordReason::InvalidJson),
    }
}

/// Treats a wall-clock deadline as the end of the readable prefix rather than
/// a failure of the records already converted.
fn soft_stop(builder: &mut DocumentBuilder<'_, '_>, stop: Stop, line: u64) -> Result<(), Stop> {
    match stop {
        Stop::OverBudget {
            kind: BudgetKind::Time,
            ..
        } => {
            builder.omit(Omission::UnreadablePart {
                detail: format!("the conversion deadline was reached at line {line}"),
            });
            Ok(())
        }
        other => Err(other),
    }
}

/// Converts a JSON Lines stream into one unit per record.
pub(crate) fn convert(
    builder: &mut DocumentBuilder<'_, '_>,
    reader: &mut dyn Read,
    window_bytes: usize,
    origin: JsonlOrigin,
) -> Result<JsonlStats, JsonlError> {
    let mut lines = LineReader::new(reader, window_bytes, MAX_LINE_BYTES, origin);
    let mut buffer = Vec::new();
    let mut constants = ConstantFilter::default();
    let mut stats = JsonlStats::default();
    let mut omitted_blobs = 0_u64;
    let mut first = true;

    loop {
        let line_number = lines.upcoming_line();
        if let Err(stop) = builder.checkpoint() {
            soft_stop(builder, stop, line_number)?;
            break;
        }
        let Some(line) = lines.next_line(&mut buffer)? else {
            break;
        };
        if first {
            first = false;
            if buffer
                .iter()
                .take(BINARY_SNIFF_BYTES)
                .any(|byte| *byte == 0)
            {
                return Err(JsonlError::Binary);
            }
            if line.offset == 0 && buffer.starts_with(b"\xEF\xBB\xBF") {
                buffer.drain(..3);
            }
        }
        let skip = if line.overlong || buffer == OVERLONG_LINE_PLACEHOLDER {
            Some(SkippedRecordReason::LineTooLong)
        } else {
            match parse_line(&buffer, &mut constants) {
                Ok(None) => None,
                Ok(Some(record)) => {
                    omitted_blobs += record.omitted_blobs;
                    match record.into_draft(line.number, line.offset) {
                        Some(draft) => {
                            match builder.push(draft) {
                                Ok(true) => stats.records += 1,
                                Ok(false) => {}
                                Err(stop) => {
                                    soft_stop(builder, stop, line.number)?;
                                    break;
                                }
                            }
                            if builder.is_saturated() {
                                break;
                            }
                            None
                        }
                        None => Some(SkippedRecordReason::NoTextFields),
                    }
                }
                Err(reason) => Some(reason),
            }
        };
        if let Some(reason) = skip {
            stats.skipped += 1;
            stats.first_skipped_line.get_or_insert(line.number);
            if stats.skipped as usize <= MAX_ITEMIZED_SKIPS {
                builder.warn(ConversionWarning::SkippedRecord {
                    line: line.number,
                    reason,
                });
            }
        }
    }
    stats.bytes_read = lines.bytes_read();
    if stats.skipped > 0 {
        builder.warn(ConversionWarning::SkippedRecords {
            count: stats.skipped,
        });
    }
    if omitted_blobs > 0 {
        builder.warn(ConversionWarning::UnsupportedFeature {
            detail: format!("{omitted_blobs} opaque binary or base64 values were omitted"),
        });
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;
    use std::time::Duration;

    use super::*;
    use crate::budget::{BudgetTracker, ConversionBudgets, ManualClock};
    use crate::cancellation::{Cancellation, CancellationFlag};
    use crate::model::{ComponentVersion, ConvertedDocument, FormatKind};

    struct Outcome {
        document: ConvertedDocument,
        stats: JsonlStats,
    }

    fn run(
        source: &[u8],
        window_bytes: usize,
        budgets: &ConversionBudgets,
        cancellation: &Cancellation,
        clock: &ManualClock,
    ) -> Result<Outcome, JsonlError> {
        let mut tracker = BudgetTracker::new(budgets, cancellation, clock);
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Jsonl,
            &mut tracker,
        );
        let mut reader = Cursor::new(source.to_vec());
        let stats = convert(&mut builder, &mut reader, window_bytes, JsonlOrigin::START)?;
        Ok(Outcome {
            document: builder.finish(),
            stats,
        })
    }

    fn convert_with(source: &str, window_bytes: usize) -> Outcome {
        run(
            source.as_bytes(),
            window_bytes,
            &ConversionBudgets::default(),
            &Cancellation::none(),
            &ManualClock::new(),
        )
        .expect("conversion")
    }

    fn lines_of(source: &[u8], window_bytes: usize) -> Vec<(u64, u64, bool, Vec<u8>)> {
        let mut reader = Cursor::new(source.to_vec());
        let mut lines = LineReader::new(&mut reader, window_bytes, 64, JsonlOrigin::START);
        let mut buffer = Vec::new();
        let mut result = Vec::new();
        while let Some(line) = lines.next_line(&mut buffer).expect("read") {
            result.push((line.number, line.offset, line.overlong, buffer.clone()));
        }
        result
    }

    #[test]
    fn a_record_spanning_the_window_edge_is_reassembled_with_absolute_positions() {
        let source = b"alpha\nbravo-charlie\ndelta\n";
        for window in [1, 2, 3, 5, 6, 7, 64] {
            let lines = lines_of(source, window);
            assert_eq!(
                lines,
                vec![
                    (1, 0, false, b"alpha".to_vec()),
                    (2, 6, false, b"bravo-charlie".to_vec()),
                    (3, 20, false, b"delta".to_vec()),
                ],
                "window {window}"
            );
        }
    }

    #[test]
    fn crlf_missing_trailing_newline_and_empty_lines_keep_line_numbers_exact() {
        let source = b"one\r\n\r\ntwo\n\nthree";
        for window in [1, 4, 64] {
            let lines = lines_of(source, window);
            assert_eq!(
                lines,
                vec![
                    (1, 0, false, b"one".to_vec()),
                    (2, 5, false, Vec::new()),
                    (3, 7, false, b"two".to_vec()),
                    (4, 11, false, Vec::new()),
                    (5, 12, false, b"three".to_vec()),
                ],
                "window {window}"
            );
        }
    }

    #[test]
    fn an_empty_stream_and_a_lone_newline_are_distinguished() {
        assert!(lines_of(b"", 8).is_empty());
        assert_eq!(lines_of(b"\n", 8), vec![(1, 0, false, Vec::new())]);
    }

    #[test]
    fn an_overlong_line_is_drained_without_losing_the_lines_after_it() {
        let mut source = vec![b'x'; 200];
        source.extend_from_slice(b"\nshort\n");
        for window in [3, 64, 1024] {
            let lines = lines_of(&source, window);
            assert_eq!(lines.len(), 2, "window {window}");
            assert_eq!((lines[0].0, lines[0].1, lines[0].2), (1, 0, true));
            assert!(lines[0].3.is_empty());
            assert_eq!(lines[1], (2, 201, false, b"short".to_vec()));
        }
    }

    #[test]
    fn known_fields_become_a_unit_with_metadata_in_the_section_path_only() {
        let outcome = convert_with(
            concat!(
                r#"{"title":"Dam breach","text":"Water rose overnight.","summary":"Flood news","#,
                r#""published":"2025-03-01","channel":"News24"}"#,
                "\n"
            ),
            WINDOW_BYTES,
        );
        let units = outcome.document.units();
        assert_eq!(units.len(), 1);
        assert_eq!(
            units[0].section_path,
            ["Dam breach", "News24 \u{b7} 2025-03-01"]
        );
        assert_eq!(units[0].text, "Flood news\n\nWater rose overnight.");
        assert!(!units[0].text.contains("News24"));
        assert!(!units[0].text.contains("2025"));
        assert_eq!(
            units[0].provenance,
            Provenance::JsonlRecord {
                line: 1,
                byte_offset: 0
            }
        );
    }

    #[test]
    fn field_names_match_case_and_separator_insensitively_and_epochs_become_dates() {
        let outcome = convert_with(
            r#"{"Channel_Name":"Radio","Published-At":1700000000,"BODY":"hello"}"#,
            WINDOW_BYTES,
        );
        let unit = &outcome.document.units()[0];
        assert_eq!(unit.section_path, ["Radio \u{b7} 2023-11-14"]);
        assert_eq!(unit.text, "hello");
        assert_eq!(epoch_date(1_700_000_000_000), Some("2023-11-14".to_owned()));
        assert_eq!(epoch_date(0), Some("1970-01-01".to_owned()));
        assert_eq!(epoch_date(-86_400), Some("1969-12-31".to_owned()));
    }

    #[test]
    fn a_title_only_record_stays_searchable() {
        let outcome = convert_with(r#"{"title":"Only a headline","channel":"A"}"#, WINDOW_BYTES);
        let unit = &outcome.document.units()[0];
        assert_eq!(unit.text, "Only a headline");
        assert_eq!(unit.section_path, ["A"]);
    }

    #[test]
    fn base64_and_data_uri_values_are_omitted_and_counted() {
        let blob = "QUJD".repeat(200);
        let source = format!(
            "{{\"text\":\"caption\",\"image\":\"{blob}\",\"thumb\":\"data:image/png;base64,AAAA\"}}\n\
             {{\"text\":\"{blob}\",\"title\":\"Blob text\"}}\n\
             {{\"kind\":\"photo\",\"image\":\"{blob}\"}}\n"
        );
        let outcome = convert_with(&source, WINDOW_BYTES);
        let units = outcome.document.units();
        assert_eq!(units.len(), 3);
        assert_eq!(units[0].text, "caption");
        assert!(units.iter().all(|unit| !unit.text.contains("QUJD")));
        assert_eq!(units[1].text, "Blob text");
        assert_eq!(units[2].text, "kind: photo");
        assert!(outcome.document.warnings().iter().any(|warning| matches!(
            warning,
            ConversionWarning::UnsupportedFeature { detail } if detail.starts_with("2 opaque")
        )));
    }

    #[test]
    fn long_prose_is_not_mistaken_for_a_blob() {
        let prose = "the quick brown fox ".repeat(40);
        assert!(!is_opaque_blob(&prose));
        assert!(is_opaque_blob(&"abcd".repeat(100)));
        assert!(!is_opaque_blob("short"));
        assert!(is_opaque_blob("data:image/png;base64,AAAA"));
    }

    #[test]
    fn records_without_known_fields_keep_short_strings_and_drop_repeated_constants() {
        let outcome = convert_with(
            concat!(
                r#"{"kind":"event","note":"first happening","count":3}"#,
                "\n",
                r#"{"kind":"event","note":"second happening"}"#,
                "\n",
                r#"{"kind":"audit","note":"second happening"}"#,
                "\n"
            ),
            WINDOW_BYTES,
        );
        let texts = outcome
            .document
            .units()
            .iter()
            .map(|unit| unit.text.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            texts,
            [
                "kind: event\nnote: first happening",
                "note: second happening",
                "kind: audit",
            ]
        );
    }

    #[test]
    fn long_unknown_values_are_not_indexed_as_fallback_fields() {
        let long = "word ".repeat(100);
        let source = format!("{{\"note\":\"{long}\",\"tag\":\"kept\"}}");
        let outcome = convert_with(&source, WINDOW_BYTES);
        assert_eq!(outcome.document.units()[0].text, "tag: kept");
    }

    #[test]
    fn malformed_lines_are_skipped_with_typed_warnings_and_exact_line_numbers() {
        let mut source = Vec::new();
        source.extend_from_slice(b"{\"text\":\"first\"}\n");
        source.extend_from_slice(b"not json\n");
        source.extend_from_slice(b"[1,2]\n");
        source.extend_from_slice(b"{\"text\":\"\xff\xfe\"}\n");
        source.extend_from_slice(b"{\"nothing\":1}\n");
        source.extend_from_slice(b"\n");
        source.extend_from_slice(b"{\"text\":\"last\"}\n");
        let outcome = run(
            &source,
            5,
            &ConversionBudgets::default(),
            &Cancellation::none(),
            &ManualClock::new(),
        )
        .expect("conversion");
        let units = outcome.document.units();
        assert_eq!(units.len(), 2);
        assert_eq!(
            units[1].provenance,
            Provenance::JsonlRecord {
                line: 7,
                byte_offset: source.len() as u64 - 16
            }
        );
        let skipped = outcome
            .document
            .warnings()
            .iter()
            .filter_map(|warning| match warning {
                ConversionWarning::SkippedRecord { line, reason } => Some((*line, *reason)),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            skipped,
            [
                (2, SkippedRecordReason::InvalidJson),
                (3, SkippedRecordReason::NotAnObject),
                (4, SkippedRecordReason::InvalidUtf8),
                (5, SkippedRecordReason::NoTextFields),
            ]
        );
        assert!(
            outcome
                .document
                .warnings()
                .contains(&ConversionWarning::SkippedRecords { count: 4 })
        );
        assert_eq!(outcome.stats.skipped, 4);
        assert_eq!(outcome.stats.first_skipped_line, Some(2));
        assert_eq!(outcome.stats.records, 2);
    }

    #[test]
    fn itemized_skips_are_bounded_but_the_total_is_exact() {
        let source = "bad\n".repeat(MAX_ITEMIZED_SKIPS + 10);
        let outcome = convert_with(&source, WINDOW_BYTES);
        let itemized = outcome
            .document
            .warnings()
            .iter()
            .filter(|warning| matches!(warning, ConversionWarning::SkippedRecord { .. }))
            .count();
        assert_eq!(itemized, MAX_ITEMIZED_SKIPS);
        assert!(
            outcome
                .document
                .warnings()
                .contains(&ConversionWarning::SkippedRecords {
                    count: (MAX_ITEMIZED_SKIPS + 10) as u64
                })
        );
    }

    #[test]
    fn interleaved_unsorted_records_keep_file_order_and_independent_positions() {
        // Dates are deliberately out of order and the metadata varies by
        // record; every unit must still sit at its own line and offset.
        let records = [
            r#"{"title":"C","published":"2025-12-31","text":"c body","channel":"X"}"#,
            r#"{"text":"no metadata"}"#,
            "garbage",
            r#"{"title":"A","published":"2020-01-01","summary":"a summary","channel":"Y"}"#,
            r#"{"kind":"fallback"}"#,
            r#"{"title":"B","published":"2023-06-15","text":"b body"}"#,
        ];
        let source = records.join("\n") + "\n";
        let mut expected_offset = 0_u64;
        let mut offsets = Vec::new();
        for record in &records {
            offsets.push(expected_offset);
            expected_offset += record.len() as u64 + 1;
        }
        for window in [3, 17, WINDOW_BYTES] {
            let outcome = convert_with(&source, window);
            let found = outcome
                .document
                .units()
                .iter()
                .map(|unit| match &unit.provenance {
                    Provenance::JsonlRecord { line, byte_offset } => (*line, *byte_offset),
                    other => panic!("unexpected provenance {other:?}"),
                })
                .collect::<Vec<_>>();
            assert_eq!(
                found,
                [
                    (1, offsets[0]),
                    (2, offsets[1]),
                    (4, offsets[3]),
                    (5, offsets[4]),
                    (6, offsets[5]),
                ],
                "window {window}"
            );
            let units = outcome.document.units();
            assert_eq!(units[0].section_path, ["C", "X \u{b7} 2025-12-31"]);
            assert_eq!(units[2].section_path, ["A", "Y \u{b7} 2020-01-01"]);
            assert_eq!(units[3].text, "kind: fallback");
            assert_eq!(units[4].section_path, ["B", "2023-06-15"]);
        }
    }

    #[test]
    fn a_leading_byte_order_mark_is_ignored_but_counted_in_offsets() {
        let mut source = b"\xEF\xBB\xBF{\"text\":\"first\"}\n".to_vec();
        source.extend_from_slice(b"{\"text\":\"second\"}\n");
        let outcome = run(
            &source,
            WINDOW_BYTES,
            &ConversionBudgets::default(),
            &Cancellation::none(),
            &ManualClock::new(),
        )
        .expect("conversion");
        let units = outcome.document.units();
        assert_eq!(units.len(), 2);
        assert_eq!(
            units[1].provenance,
            Provenance::JsonlRecord {
                line: 2,
                byte_offset: 3 + 16 + 1
            }
        );
    }

    #[test]
    fn binary_content_is_refused() {
        let result = run(
            b"{\"text\":\"a\0b\"}\n",
            WINDOW_BYTES,
            &ConversionBudgets::default(),
            &Cancellation::none(),
            &ManualClock::new(),
        );
        assert!(matches!(result, Err(JsonlError::Binary)));
    }

    #[test]
    fn the_unit_budget_stops_reading_and_keeps_earlier_records() {
        let source = (1..=50)
            .map(|index| format!("{{\"text\":\"record {index}\"}}\n"))
            .collect::<String>();
        let budgets = ConversionBudgets {
            max_units: 5,
            ..ConversionBudgets::default()
        };
        let outcome = run(
            source.as_bytes(),
            WINDOW_BYTES,
            &budgets,
            &Cancellation::none(),
            &ManualClock::new(),
        )
        .expect("conversion");
        assert_eq!(outcome.document.units().len(), 5);
        assert!(outcome.document.is_partial());
        assert!(
            outcome
                .document
                .omissions()
                .contains(&Omission::UnitLimit { limit: 5 })
        );
        // Reading stopped early rather than consuming all 50 lines.
        assert!(outcome.stats.bytes_read < source.len() as u64);
    }

    #[test]
    fn the_output_budget_truncates_with_a_typed_omission() {
        let source = (1..=20)
            .map(|index| format!("{{\"text\":\"{}\"}}\n", "x".repeat(40 + index)))
            .collect::<String>();
        let budgets = ConversionBudgets {
            max_output_chars: 120,
            ..ConversionBudgets::default()
        };
        let outcome = run(
            source.as_bytes(),
            WINDOW_BYTES,
            &budgets,
            &Cancellation::none(),
            &ManualClock::new(),
        )
        .expect("conversion");
        assert!(outcome.document.is_partial());
        assert!(
            outcome
                .document
                .omissions()
                .contains(&Omission::OutputCharLimit { limit: 120 })
        );
        assert!(!outcome.document.units().is_empty());
    }

    #[test]
    fn a_deadline_keeps_the_records_converted_before_it() {
        let budgets = ConversionBudgets {
            timeout: Duration::from_millis(10),
            ..ConversionBudgets::default()
        };
        let clock = ManualClock::new();
        let cancellation = Cancellation::none();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Jsonl,
            &mut tracker,
        );
        // A reader that advances the clock past the deadline once the first
        // window has been served.
        struct SlowReader<'c> {
            data: Cursor<Vec<u8>>,
            clock: &'c ManualClock,
        }
        impl Read for SlowReader<'_> {
            fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
                let read = self.data.read(buffer)?;
                self.clock.advance(Duration::from_millis(11));
                Ok(read)
            }
        }
        let mut reader = SlowReader {
            data: Cursor::new(b"{\"text\":\"a\"}\n{\"text\":\"b\"}\n{\"text\":\"c\"}\n".to_vec()),
            clock: &clock,
        };
        convert(&mut builder, &mut reader, 13, JsonlOrigin::START).expect("conversion");
        let document = builder.finish();
        assert!(document.is_partial());
        assert!(document.omissions().iter().any(
            |omission| matches!(omission, Omission::UnreadablePart { detail } if detail.contains("deadline"))
        ));
        assert!(document.units().len() < 3);
    }

    #[test]
    fn cancellation_stops_conversion() {
        let flag = CancellationFlag::new();
        flag.cancel();
        let result = run(
            b"{\"text\":\"a\"}\n",
            WINDOW_BYTES,
            &ConversionBudgets::default(),
            &flag.handle(),
            &ManualClock::new(),
        );
        assert!(matches!(result, Err(JsonlError::Stopped(Stop::Cancelled))));
    }
}
