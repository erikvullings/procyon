//! Shared ZIP package handling: preflight, bounded part reads and XML safety.
//!
//! Everything below reads *only* XML text and structure from the package.
//! Relationship parts are not followed, no external reference is resolved, no
//! file outside the package is opened and no network request is made, so a
//! hostile document cannot turn conversion into a fetch primitive.
//!
//! Preflight rejects, before any part is decompressed:
//!
//! * entry counts above the archive budget,
//! * total uncompressed size above the expansion budget,
//! * entries that are encrypted (password-protected packages),
//! * entry names that escape the package (`..`, absolute paths, backslashes),
//! * entries whose compression ratio marks them as a decompression bomb.

use std::collections::HashSet;
use std::io::{Cursor, Read};

use quick_xml::events::Event;

use crate::budget::{BudgetTracker, Stop};

/// Ratio above which an entry is treated as a decompression bomb. Ordinary
/// OOXML markup compresses by roughly one order of magnitude; two hundred
/// times leaves ample headroom for very repetitive spreadsheets while still
/// catching a zero-filled bomb, which deflate compresses by about 1000x.
const MAX_COMPRESSION_RATIO: u64 = 200;

/// Size below which the ratio check is not applied, so that tiny, highly
/// compressible XML parts are not mistaken for bombs.
const RATIO_CHECK_FLOOR: u64 = 1024 * 1024;

/// Why a package could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum PackageError {
    /// The container or a part is structurally broken.
    Malformed(String),
    /// The package is encrypted or password protected.
    Encrypted(String),
    /// A hard budget stopped the work, or the caller cancelled it.
    Stopped(Stop),
}

impl From<Stop> for PackageError {
    fn from(stop: Stop) -> Self {
        Self::Stopped(stop)
    }
}

pub(crate) type Package<'a> = zip::ZipArchive<Cursor<&'a [u8]>>;

/// Validates the package and returns an archive positioned for part reads.
pub(crate) fn preflight<'a>(
    bytes: &'a [u8],
    tracker: &mut BudgetTracker<'_>,
) -> Result<Package<'a>, PackageError> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|error| {
        PackageError::Malformed(format!("the package is not readable: {error}"))
    })?;
    tracker.charge_archive_entries(archive.len() as u64)?;
    let mut names = HashSet::new();
    for index in 0..archive.len() {
        tracker.checkpoint()?;
        let entry = archive.by_index_raw(index).map_err(|error| {
            PackageError::Malformed(format!("package entry {index} is unreadable: {error}"))
        })?;
        let name = entry.name().to_owned();
        if entry.encrypted() {
            return Err(PackageError::Encrypted(format!(
                "package entry '{name}' is encrypted"
            )));
        }
        if let Some(detail) = unsafe_entry_detail(
            &name,
            entry.enclosed_name().is_some(),
            entry.size(),
            entry.compressed_size(),
        ) {
            return Err(PackageError::Malformed(detail));
        }
        if !names.insert(name.clone()) {
            return Err(PackageError::Malformed(format!(
                "package contains duplicate entry '{name}'"
            )));
        }
        let uncompressed = entry.size();
        drop(entry);
        tracker.charge_expanded_bytes(uncompressed)?;
    }
    Ok(archive)
}

/// Names of the entries in the package, in archive order.
pub(crate) fn part_names(archive: &Package<'_>) -> Vec<String> {
    archive.file_names().map(str::to_owned).collect()
}

/// Reads one part, bounded by `limit` bytes.
pub(crate) fn read_part(
    archive: &mut Package<'_>,
    name: &str,
    limit: u64,
) -> Result<Option<Vec<u8>>, PackageError> {
    let mut entry = match archive.by_name(name) {
        Ok(entry) => entry,
        Err(zip::result::ZipError::FileNotFound) => return Ok(None),
        Err(zip::result::ZipError::UnsupportedArchive(detail))
            if detail.contains("Password") || detail.contains("password") =>
        {
            return Err(PackageError::Encrypted(format!(
                "part '{name}' requires a password"
            )));
        }
        Err(error) => {
            return Err(PackageError::Malformed(format!(
                "part '{name}' is unreadable: {error}"
            )));
        }
    };
    let mut buffer = Vec::new();
    entry
        .by_ref()
        .take(limit.saturating_add(1))
        .read_to_end(&mut buffer)
        .map_err(|error| {
            PackageError::Malformed(format!("part '{name}' is unreadable: {error}"))
        })?;
    if buffer.len() as u64 > limit {
        return Err(PackageError::Stopped(Stop::OverBudget {
            kind: crate::budget::BudgetKind::PartBytes,
            limit,
        }));
    }
    Ok(Some(buffer))
}

pub(crate) fn unsafe_entry_detail(
    name: &str,
    enclosed: bool,
    uncompressed: u64,
    compressed: u64,
) -> Option<String> {
    if !enclosed || name.contains('\\') {
        return Some(format!("package entry '{name}' escapes the package"));
    }
    if uncompressed > RATIO_CHECK_FLOOR && uncompressed / compressed.max(1) > MAX_COMPRESSION_RATIO
    {
        return Some(format!(
            "package entry '{name}' expands by more than {MAX_COMPRESSION_RATIO}x"
        ));
    }
    None
}

/// Creates an XML reader with the settings every package parser here uses.
pub(crate) fn xml_reader(bytes: &[u8]) -> quick_xml::Reader<&[u8]> {
    let mut reader = quick_xml::Reader::from_reader(bytes);
    reader.config_mut().trim_text(false);
    // End-name checking is what turns a truncated or mismatched part into a
    // typed malformed outcome instead of silently short output.
    reader.config_mut().check_end_names = true;
    reader
}

/// Local element name of an XML event name, with any namespace prefix
/// removed.
pub(crate) fn local_name(name: &[u8]) -> String {
    let text = String::from_utf8_lossy(name);
    text.rsplit(':').next().unwrap_or_default().to_owned()
}

/// Applies the shared structural rules to one XML event: DOCTYPE declarations
/// are refused outright (they are the entry point for entity expansion) and
/// element nesting is charged against the depth budget.
pub(crate) fn inspect_event(
    event: &Event<'_>,
    depth: &mut u32,
    tracker: &BudgetTracker<'_>,
) -> Result<(), PackageError> {
    match event {
        Event::DocType(_) => Err(PackageError::Malformed(
            "DOCTYPE declarations are not allowed in package XML parts".to_owned(),
        )),
        Event::Start(_) => {
            *depth += 1;
            tracker.charge_depth(*depth).map_err(PackageError::from)
        }
        Event::End(_) => {
            *depth = depth.saturating_sub(1);
            Ok(())
        }
        _ => Ok(()),
    }
}

/// Fails when a part ended with elements still open, which `quick-xml` itself
/// reports as a plain end of input.
pub(crate) fn ensure_balanced(depth: u32, name: &str) -> Result<(), PackageError> {
    if depth == 0 {
        return Ok(());
    }
    Err(PackageError::Malformed(format!(
        "part '{name}' ended with {depth} unclosed element(s)"
    )))
}

#[cfg(test)]
pub(crate) mod tests {
    use std::io::Write;

    use zip::write::SimpleFileOptions;

    use super::*;
    use crate::budget::{ConversionBudgets, ManualClock};
    use crate::cancellation::Cancellation;

    /// Builds a small ZIP package from in-memory parts.
    pub(crate) fn package(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut buffer = Cursor::new(Vec::new());
        {
            let mut archive = zip::ZipWriter::new(&mut buffer);
            for (name, data) in entries {
                archive
                    .start_file(*name, SimpleFileOptions::default())
                    .expect("start entry");
                archive.write_all(data).expect("write entry");
            }
            archive.finish().expect("finish archive");
        }
        buffer.into_inner()
    }

    #[test]
    fn a_broken_container_is_malformed() {
        let budgets = ConversionBudgets::default();
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let outcome = preflight(b"PK\x03\x04not a zip", &mut tracker);
        assert!(matches!(outcome, Err(PackageError::Malformed(_))));
    }

    #[test]
    fn the_entry_budget_stops_preflight() {
        let entries: Vec<(String, Vec<u8>)> = (0..8)
            .map(|index| (format!("part{index}.xml"), b"<a/>".to_vec()))
            .collect();
        let borrowed: Vec<(&str, &[u8])> = entries
            .iter()
            .map(|(name, data)| (name.as_str(), data.as_slice()))
            .collect();
        let bytes = package(&borrowed);
        let budgets = ConversionBudgets {
            max_archive_entries: 4,
            ..ConversionBudgets::default()
        };
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        assert!(matches!(
            preflight(&bytes, &mut tracker),
            Err(PackageError::Stopped(Stop::OverBudget {
                kind: crate::budget::BudgetKind::ArchiveEntries,
                ..
            }))
        ));
    }

    #[test]
    fn the_expansion_budget_stops_preflight() {
        let bytes = package(&[("big.xml", vec![b'a'; 4096].as_slice())]);
        let budgets = ConversionBudgets {
            max_expanded_bytes: 1024,
            ..ConversionBudgets::default()
        };
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        assert!(matches!(
            preflight(&bytes, &mut tracker),
            Err(PackageError::Stopped(Stop::OverBudget {
                kind: crate::budget::BudgetKind::ExpandedBytes,
                ..
            }))
        ));
    }

    #[test]
    fn a_decompression_bomb_is_refused_before_any_part_is_read() {
        let bomb = vec![0_u8; 2 * 1024 * 1024];
        let bytes = package(&[("word/document.xml", bomb.as_slice())]);
        assert!(
            (bytes.len() as u64) < 64 * 1024,
            "the fixture must actually be a bomb"
        );
        let budgets = ConversionBudgets::default();
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        assert!(matches!(
            preflight(&bytes, &mut tracker),
            Err(PackageError::Malformed(_))
        ));
    }

    #[test]
    fn a_traversing_entry_name_is_refused() {
        let bytes = package(&[("../escape.xml", b"<a/>")]);
        let budgets = ConversionBudgets::default();
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        assert!(matches!(
            preflight(&bytes, &mut tracker),
            Err(PackageError::Malformed(_))
        ));
    }

    #[test]
    fn parts_are_listed_and_read_within_a_limit() {
        let bytes = package(&[("word/document.xml", b"<w:document/>")]);
        let budgets = ConversionBudgets::default();
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut archive = preflight(&bytes, &mut tracker).expect("preflight");
        assert_eq!(part_names(&archive), ["word/document.xml"]);
        let part = read_part(&mut archive, "word/document.xml", 1024)
            .expect("read")
            .expect("present");
        assert_eq!(part, b"<w:document/>");
        assert_eq!(
            read_part(&mut archive, "word/missing.xml", 1024).expect("read"),
            None
        );
    }

    #[test]
    fn an_oversized_part_is_reported_instead_of_silently_truncated() {
        let bytes = package(&[("word/document.xml", b"<w:document>text</w:document>")]);
        let budgets = ConversionBudgets::default();
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut archive = preflight(&bytes, &mut tracker).expect("preflight");

        assert!(matches!(
            read_part(&mut archive, "word/document.xml", 8),
            Err(PackageError::Stopped(Stop::OverBudget {
                kind: crate::budget::BudgetKind::PartBytes,
                limit: 8
            }))
        ));
    }

    #[test]
    fn an_unbalanced_part_is_malformed() {
        assert_eq!(ensure_balanced(0, "word/document.xml"), Ok(()));
        assert!(matches!(
            ensure_balanced(2, "word/document.xml"),
            Err(PackageError::Malformed(_))
        ));
    }

    #[test]
    fn a_doctype_declaration_is_refused() {
        let budgets = ConversionBudgets::default();
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut depth = 0;
        let event = Event::DocType(quick_xml::events::BytesText::new("doc"));
        assert!(matches!(
            inspect_event(&event, &mut depth, &tracker),
            Err(PackageError::Malformed(_))
        ));
    }
}
