//! WordprocessingML conversion.
//!
//! Reads `word/document.xml` only: paragraphs, their outline level or heading
//! style, and tables. Pagination is deliberately absent - page numbers depend
//! on a layout engine Procyon does not run, so units report a body block index
//! instead of an invented page.

use quick_xml::events::Event;

use crate::builder::{DocumentBuilder, UnitDraft};
use crate::formats::ooxml::{self, Package, PackageError};
use crate::model::{Omission, Provenance, TopLevelBoundary, UnitKind};

/// Maximum bytes read from `word/document.xml`.
const MAX_DOCUMENT_PART_BYTES: u64 = 64 * 1024 * 1024;

/// Heading level implied by a paragraph style name, if any.
fn style_heading_level(style: &str) -> Option<u32> {
    let normalized = style
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-', '_'], "");
    if normalized == "title" {
        return Some(1);
    }
    let rest = normalized.strip_prefix("heading")?;
    rest.parse::<u32>().ok().filter(|level| *level >= 1)
}

struct TableState {
    rows: Vec<Vec<String>>,
    cell: String,
}

/// Converts a DOCX package into headings, paragraphs and tables.
pub(crate) fn convert(
    builder: &mut DocumentBuilder<'_, '_>,
    archive: &mut Package<'_>,
) -> Result<(), PackageError> {
    let Some(part) = ooxml::read_part(archive, "word/document.xml", MAX_DOCUMENT_PART_BYTES)?
    else {
        return Err(PackageError::Malformed(
            "the package has no word/document.xml part".to_owned(),
        ));
    };
    let mut reader = ooxml::xml_reader(&part);
    let mut buffer = Vec::new();
    let mut depth = 0_u32;

    let mut paragraph = String::new();
    let mut style: Option<String> = None;
    let mut in_paragraph_properties = false;
    let mut table: Option<TableState> = None;
    let mut block_index = 0_u32;
    let mut headings: Vec<(u32, String)> = Vec::new();

    loop {
        builder.checkpoint().map_err(PackageError::from)?;
        if builder.is_saturated() {
            break;
        }
        let event = reader.read_event_into(&mut buffer).map_err(|error| {
            PackageError::Malformed(format!("word/document.xml is not well formed: {error}"))
        })?;
        ooxml::inspect_event(&event, &mut depth, builder.tracker())?;
        match &event {
            Event::Eof => break,
            Event::Start(start) => match ooxml::local_name(start.name().as_ref()).as_str() {
                "tbl" => {
                    table = Some(TableState {
                        rows: Vec::new(),
                        cell: String::new(),
                    });
                }
                "tr" => {
                    if let Some(state) = table.as_mut() {
                        state.rows.push(Vec::new());
                    }
                }
                "pPr" => in_paragraph_properties = true,
                "p" => {
                    paragraph.clear();
                    style = None;
                }
                _ => {}
            },
            Event::Empty(empty) => match ooxml::local_name(empty.name().as_ref()).as_str() {
                "pStyle" if in_paragraph_properties => {
                    style = empty.attributes().flatten().find_map(|attribute| {
                        (ooxml::local_name(attribute.key.as_ref()) == "val")
                            .then(|| String::from_utf8_lossy(&attribute.value).into_owned())
                    });
                }
                "outlineLvl" if in_paragraph_properties && style.is_none() => {
                    if let Some(level) = empty.attributes().flatten().find_map(|attribute| {
                        (ooxml::local_name(attribute.key.as_ref()) == "val")
                            .then(|| String::from_utf8_lossy(&attribute.value).into_owned())
                    }) && let Ok(level) = level.parse::<u32>()
                    {
                        style = Some(format!("Heading{}", level + 1));
                    }
                }
                "tab" => push_text(&mut paragraph, &mut table, "\t"),
                "br" | "cr" => push_text(&mut paragraph, &mut table, "\n"),
                _ => {}
            },
            Event::Text(text) => {
                let decoded = text.decode().map_err(|error| {
                    PackageError::Malformed(format!("word/document.xml has invalid text: {error}"))
                })?;
                if !decoded.is_empty() {
                    push_text(&mut paragraph, &mut table, &decoded);
                }
            }
            Event::End(end) => match ooxml::local_name(end.name().as_ref()).as_str() {
                "pPr" => in_paragraph_properties = false,
                "tc" => {
                    if let Some(state) = table.as_mut() {
                        let cell = std::mem::take(&mut state.cell);
                        if let Some(row) = state.rows.last_mut() {
                            row.push(cell.trim().to_owned());
                        }
                    }
                }
                "p" => {
                    if let Some(state) = table.as_mut() {
                        if !state.cell.is_empty() && !state.cell.ends_with(' ') {
                            state.cell.push(' ');
                        }
                    } else {
                        let text = std::mem::take(&mut paragraph);
                        let trimmed = text.trim();
                        if !trimmed.is_empty() {
                            emit_paragraph(
                                builder,
                                trimmed,
                                style.as_deref(),
                                block_index,
                                &mut headings,
                            )?;
                        }
                        block_index += 1;
                    }
                }
                "tbl" => {
                    if let Some(state) = table.take() {
                        emit_table(builder, &state, block_index, &headings)?;
                        block_index += 1;
                    }
                }
                _ => {}
            },
            _ => {}
        }
        buffer.clear();
    }

    if builder.is_saturated() {
        builder.omit(Omission::UnreadablePart {
            detail: "the document body was truncated by an output budget".to_owned(),
        });
    } else {
        ooxml::ensure_balanced(depth, "word/document.xml")?;
    }
    Ok(())
}

fn push_text(paragraph: &mut String, table: &mut Option<TableState>, value: &str) {
    match table.as_mut() {
        Some(state) => state.cell.push_str(value),
        None => paragraph.push_str(value),
    }
}

fn section_path(headings: &[(u32, String)]) -> Vec<String> {
    headings.iter().map(|(_, title)| title.clone()).collect()
}

fn boundary(headings: &[(u32, String)]) -> TopLevelBoundary {
    headings
        .first()
        .map_or(TopLevelBoundary::Document, |(_, title)| {
            TopLevelBoundary::Section(title.clone())
        })
}

fn emit_paragraph(
    builder: &mut DocumentBuilder<'_, '_>,
    text: &str,
    style: Option<&str>,
    block_index: u32,
    headings: &mut Vec<(u32, String)>,
) -> Result<(), PackageError> {
    let level = style.and_then(style_heading_level);
    // A heading opens its own section: rewind the stack before emitting so the
    // heading's boundary is the section it introduces.
    if let Some(level) = level {
        while headings.last().is_some_and(|(open, _)| *open >= level) {
            headings.pop();
        }
    }
    let path = section_path(headings);
    let unit_boundary = match (level, headings.first()) {
        (_, Some((_, title))) => TopLevelBoundary::Section(title.clone()),
        (Some(_), None) => TopLevelBoundary::Section(text.to_owned()),
        (None, None) => TopLevelBoundary::Document,
    };
    builder
        .push(UnitDraft {
            kind: if level.is_some() {
                UnitKind::Heading
            } else {
                UnitKind::Paragraph
            },
            section_path: path,
            text: text.to_owned(),
            provenance: Provenance::DocxBlock { block_index },
            boundary: unit_boundary,
            source_offset: None,
        })
        .map_err(PackageError::from)?;
    if let Some(level) = level {
        headings.push((level, text.to_owned()));
    }
    Ok(())
}

fn emit_table(
    builder: &mut DocumentBuilder<'_, '_>,
    state: &TableState,
    block_index: u32,
    headings: &[(u32, String)],
) -> Result<(), PackageError> {
    let text = state
        .rows
        .iter()
        .map(|row| row.join(" | "))
        .filter(|row| !row.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        return Ok(());
    }
    builder
        .push(UnitDraft {
            kind: UnitKind::Table,
            section_path: section_path(headings),
            text,
            provenance: Provenance::DocxBlock { block_index },
            boundary: boundary(headings),
            source_offset: None,
        })
        .map_err(PackageError::from)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::budget::{BudgetTracker, ConversionBudgets, ManualClock, Stop};
    use crate::cancellation::{Cancellation, CancellationFlag};
    use crate::formats::ooxml::tests::package;
    use crate::model::{ComponentVersion, ConvertedDocument, FormatKind};

    const DOCUMENT: &[u8] = br#"<?xml version="1.0"?>
<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main">
  <w:body>
    <w:p><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Chapter One</w:t></w:r></w:p>
    <w:p><w:r><w:t>Opening </w:t></w:r><w:r><w:t>paragraph.</w:t></w:r></w:p>
    <w:p><w:pPr><w:pStyle w:val="Heading2"/></w:pPr><w:r><w:t>Details</w:t></w:r></w:p>
    <w:p><w:r><w:t>Detail body.</w:t></w:r></w:p>
    <w:tbl>
      <w:tr><w:tc><w:p><w:r><w:t>Region</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>Total</w:t></w:r></w:p></w:tc></w:tr>
      <w:tr><w:tc><w:p><w:r><w:t>North</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>12</w:t></w:r></w:p></w:tc></w:tr>
    </w:tbl>
  </w:body>
</w:document>"#;

    fn docx_package() -> Vec<u8> {
        package(&[
            (
                "[Content_Types].xml",
                br#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"/>"#,
            ),
            ("word/document.xml", DOCUMENT),
        ])
    }

    fn convert_package(bytes: &[u8]) -> Result<ConvertedDocument, PackageError> {
        convert_package_with(bytes, ConversionBudgets::default(), Cancellation::none())
    }

    fn convert_package_with(
        bytes: &[u8],
        budgets: ConversionBudgets,
        cancellation: Cancellation,
    ) -> Result<ConvertedDocument, PackageError> {
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut archive = ooxml::preflight(bytes, &mut tracker)?;
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Docx,
            &mut tracker,
        );
        convert(&mut builder, &mut archive)?;
        Ok(builder.finish())
    }

    #[test]
    fn headings_paragraphs_and_tables_are_extracted_in_body_order() {
        let document = convert_package(&docx_package()).expect("conversion");
        let texts: Vec<&str> = document
            .units()
            .iter()
            .map(|unit| unit.text.as_str())
            .collect();
        assert_eq!(
            texts,
            [
                "Chapter One",
                "Opening paragraph.",
                "Details",
                "Detail body.",
                "Region | Total\nNorth | 12"
            ]
        );
        assert_eq!(document.units()[3].section_path, ["Chapter One", "Details"]);
        assert_eq!(document.units()[4].kind, UnitKind::Table);
        assert_eq!(
            document.units()[4].provenance,
            Provenance::DocxBlock { block_index: 4 }
        );
        assert_eq!(
            document.units()[0].boundary,
            TopLevelBoundary::Section("Chapter One".to_owned())
        );
    }

    #[test]
    fn outline_levels_are_honoured_when_no_style_is_present() {
        let bytes = package(&[(
            "word/document.xml",
            br#"<w:document xmlns:w="x"><w:body>
              <w:p><w:pPr><w:outlineLvl w:val="0"/></w:pPr><w:r><w:t>Outlined</w:t></w:r></w:p>
              <w:p><w:r><w:t>Body</w:t></w:r></w:p>
            </w:body></w:document>"#,
        )]);
        let document = convert_package(&bytes).expect("conversion");
        assert_eq!(document.units()[0].kind, UnitKind::Heading);
        assert_eq!(document.units()[1].section_path, ["Outlined"]);
    }

    #[test]
    fn a_missing_document_part_is_malformed() {
        let bytes = package(&[("word/other.xml", b"<a/>")]);
        assert!(matches!(
            convert_package(&bytes),
            Err(PackageError::Malformed(_))
        ));
    }

    #[test]
    fn broken_xml_is_malformed() {
        let bytes = package(&[("word/document.xml", b"<w:document><w:body><w:p>")]);
        let outcome = convert_package(&bytes);
        assert!(matches!(outcome, Err(PackageError::Malformed(_))));
    }

    #[test]
    fn cancellation_stops_the_parse() {
        let flag = CancellationFlag::new();
        flag.cancel();
        assert!(matches!(
            convert_package_with(&docx_package(), ConversionBudgets::default(), flag.handle()),
            Err(PackageError::Stopped(Stop::Cancelled))
        ));
    }

    #[test]
    fn style_names_map_to_heading_levels_conservatively() {
        assert_eq!(style_heading_level("Heading1"), Some(1));
        assert_eq!(style_heading_level("heading 3"), Some(3));
        assert_eq!(style_heading_level("Title"), Some(1));
        assert_eq!(style_heading_level("BodyText"), None);
        assert_eq!(style_heading_level("HeadingChar"), None);
    }
}
