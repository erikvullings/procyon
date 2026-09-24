//! SpreadsheetML conversion.
//!
//! Calamine parses the workbook; this module only bounds it and turns cells
//! into citable bands. Sheets are visited in workbook order, rows in ascending
//! order, and every unit records the exact 0-based cell range it came from.
//! Formulas, charts and pivot caches are not interpreted. When visual evidence
//! is explicitly requested, embedded raster images referenced through the
//! workbook's worksheet/drawing relationship chain are retained separately
//! from semantic text.

use std::io::Cursor;

use calamine::{Data, Reader};

use crate::budget::Stop;
use crate::builder::{DocumentBuilder, UnitDraft, VisualDraft};
use crate::formats::csv::ROWS_PER_BAND;
use crate::formats::package::{self, BoundedPart, Package, PackageError, RelationshipTarget};
use crate::model::{Omission, Provenance, TopLevelBoundary, UnitKind, VisualProvenance};

/// Maximum columns read from one sheet.
const MAX_COLUMNS: u32 = 2_048;
const MAX_XML_PART_BYTES: u64 = 16 * 1024 * 1024;
const MAX_IMAGE_PART_BYTES: u64 = 4 * 1024 * 1024;

/// Why a workbook could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum WorkbookError {
    /// The workbook is structurally broken or unsupported.
    Malformed(String),
    /// The workbook is encrypted or password protected.
    Encrypted(String),
    /// A hard budget stopped the work, or the caller cancelled it.
    Stopped(Stop),
}

impl From<Stop> for WorkbookError {
    fn from(stop: Stop) -> Self {
        Self::Stopped(stop)
    }
}

fn cell_text(value: &Data) -> String {
    match value {
        Data::Empty => String::new(),
        other => other.to_string(),
    }
}

/// Converts an XLSX workbook into bands of rows per sheet.
pub(crate) fn convert(
    builder: &mut DocumentBuilder<'_, '_>,
    bytes: &[u8],
) -> Result<(), WorkbookError> {
    let mut workbook =
        calamine::open_workbook_auto_from_rs(Cursor::new(bytes.to_vec())).map_err(|error| {
            let detail = error.to_string();
            if detail.to_ascii_lowercase().contains("password")
                || detail.to_ascii_lowercase().contains("encrypt")
            {
                WorkbookError::Encrypted(detail)
            } else {
                WorkbookError::Malformed(format!("the workbook is not readable: {detail}"))
            }
        })?;
    let sheet_names = workbook.sheet_names().to_vec();
    if sheet_names.is_empty() {
        return Err(WorkbookError::Malformed(
            "the workbook contains no worksheets".to_owned(),
        ));
    }
    builder
        .tracker()
        .charge_items(sheet_names.len() as u64)
        .map_err(WorkbookError::from)?;

    for name in sheet_names {
        builder.checkpoint().map_err(WorkbookError::from)?;
        if builder.is_saturated() {
            break;
        }
        let range = match workbook.worksheet_range(&name) {
            Ok(range) => range,
            Err(error) => {
                builder.omit(Omission::UnreadablePart {
                    detail: format!("worksheet '{name}' could not be read: {error}"),
                });
                continue;
            }
        };
        let Some((start_row, start_column)) = range.start() else {
            continue;
        };
        let Some((end_row, end_column)) = range.end() else {
            continue;
        };
        let end_column = end_column.min(start_column.saturating_add(MAX_COLUMNS - 1));
        if end_column
            < start_column
                .saturating_add(range.width() as u32)
                .saturating_sub(1)
        {
            builder.omit(Omission::UnreadablePart {
                detail: format!("worksheet '{name}' was truncated to {MAX_COLUMNS} columns"),
            });
        }

        let mut band_start = start_row;
        while band_start <= end_row {
            builder.checkpoint().map_err(WorkbookError::from)?;
            if builder.is_saturated() {
                break;
            }
            let band_end = band_start
                .saturating_add(ROWS_PER_BAND as u32 - 1)
                .min(end_row);
            let mut lines = Vec::new();
            for row in band_start..=band_end {
                let cells: Vec<String> = (start_column..=end_column)
                    .map(|column| {
                        range
                            .get_value((row, column))
                            .map(cell_text)
                            .unwrap_or_default()
                    })
                    .collect();
                if cells.iter().any(|cell| !cell.is_empty()) {
                    lines.push(cells.join(" | "));
                }
            }
            if !lines.is_empty() {
                builder
                    .push(UnitDraft {
                        kind: UnitKind::Table,
                        section_path: vec![name.clone()],
                        text: lines.join("\n"),
                        provenance: Provenance::SpreadsheetRange {
                            sheet: name.clone(),
                            start_row: band_start,
                            start_column,
                            end_row: band_end,
                            end_column,
                        },
                        boundary: TopLevelBoundary::Sheet(name.clone()),
                        source_offset: None,
                    })
                    .map_err(WorkbookError::from)?;
            }
            band_start = band_end.saturating_add(1);
        }
    }
    Ok(())
}

pub(crate) fn extract_visuals(
    builder: &mut DocumentBuilder<'_, '_>,
    archive: &mut Package<'_>,
) -> Result<(), PackageError> {
    let Some(workbook) = package::read_part(archive, "xl/workbook.xml", MAX_XML_PART_BYTES)? else {
        return Ok(());
    };
    let workbook_relationships =
        package::read_relationships(archive, "xl/workbook.xml", MAX_XML_PART_BYTES)?;
    for (sheet_index, sheet_relationship) in relationship_ids(&workbook, "sheet", "id")
        .into_iter()
        .enumerate()
    {
        builder.checkpoint().map_err(PackageError::from)?;
        let Some(RelationshipTarget::Internal(sheet_part)) =
            workbook_relationships.get(&sheet_relationship)
        else {
            builder.omit_visual(
                Some(VisualProvenance::SpreadsheetImage {
                    sheet_index: sheet_index as u32,
                    image_index: 0,
                }),
                "spreadsheet worksheet relationship is external or missing",
            );
            continue;
        };
        let Some(sheet) = package::read_part(archive, sheet_part, MAX_XML_PART_BYTES)? else {
            continue;
        };
        let sheet_relationships =
            package::read_relationships(archive, sheet_part, MAX_XML_PART_BYTES)?;
        let mut image_index = 0_u32;
        for drawing_relationship in relationship_ids(&sheet, "drawing", "id") {
            let Some(RelationshipTarget::Internal(drawing_part)) =
                sheet_relationships.get(&drawing_relationship)
            else {
                builder.omit_visual(
                    Some(VisualProvenance::SpreadsheetImage {
                        sheet_index: sheet_index as u32,
                        image_index,
                    }),
                    "spreadsheet drawing relationship is external or missing",
                );
                continue;
            };
            let Some(drawing) = package::read_part(archive, drawing_part, MAX_XML_PART_BYTES)?
            else {
                continue;
            };
            let drawing_relationships =
                package::read_relationships(archive, drawing_part, MAX_XML_PART_BYTES)?;
            for image_relationship in image_relationship_ids(&drawing) {
                let provenance = VisualProvenance::SpreadsheetImage {
                    sheet_index: sheet_index as u32,
                    image_index,
                };
                image_index = image_index.saturating_add(1);
                let Some(RelationshipTarget::Internal(image_part)) =
                    drawing_relationships.get(&image_relationship)
                else {
                    builder.omit_visual(
                        Some(provenance),
                        "external spreadsheet images are not fetched",
                    );
                    continue;
                };
                let Some(media_type) = package::image_media_type(image_part) else {
                    builder.omit_visual(
                        Some(provenance),
                        "spreadsheet image uses an unsupported media type",
                    );
                    continue;
                };
                let data = match package::read_bounded_visual_part(
                    archive,
                    image_part,
                    MAX_IMAGE_PART_BYTES,
                )? {
                    BoundedPart::Data(data) => data,
                    BoundedPart::Missing => {
                        builder.omit_visual(Some(provenance), "spreadsheet image part is missing");
                        continue;
                    }
                    BoundedPart::TooLarge => {
                        builder.omit_visual(
                            Some(provenance),
                            format!("spreadsheet image exceeds {MAX_IMAGE_PART_BYTES} bytes"),
                        );
                        continue;
                    }
                };
                builder
                    .push_visual(VisualDraft {
                        media_type,
                        data,
                        provenance,
                        caption: None,
                    })
                    .map_err(PackageError::from)?;
            }
        }
    }
    Ok(())
}

fn image_relationship_ids(bytes: &[u8]) -> Vec<String> {
    let mut reader = package::xml_reader(bytes);
    let mut buffer = Vec::new();
    let mut ids = Vec::new();
    while let Ok(event) = reader.read_event_into(&mut buffer) {
        match &event {
            quick_xml::events::Event::Start(start) | quick_xml::events::Event::Empty(start)
                if package::local_name(start.name().as_ref()) == "blip" =>
            {
                if let Some(id) = start.attributes().flatten().find_map(|attribute| {
                    matches!(
                        package::local_name(attribute.key.as_ref()).as_str(),
                        "embed" | "link"
                    )
                    .then(|| String::from_utf8_lossy(&attribute.value).into_owned())
                }) {
                    ids.push(id);
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    ids
}

fn relationship_ids(bytes: &[u8], element: &str, attribute_name: &str) -> Vec<String> {
    let mut reader = package::xml_reader(bytes);
    let mut buffer = Vec::new();
    let mut ids = Vec::new();
    while let Ok(event) = reader.read_event_into(&mut buffer) {
        match &event {
            quick_xml::events::Event::Start(start) | quick_xml::events::Event::Empty(start)
                if package::local_name(start.name().as_ref()) == element =>
            {
                if let Some(id) = start.attributes().flatten().find_map(|attribute| {
                    (package::local_name(attribute.key.as_ref()) == attribute_name)
                        .then(|| String::from_utf8_lossy(&attribute.value).into_owned())
                }) {
                    ids.push(id);
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buffer.clear();
    }
    ids
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::budget::{BudgetTracker, ConversionBudgets, ManualClock};
    use crate::cancellation::Cancellation;
    use crate::formats::package::tests::{package, png};
    use crate::model::{ComponentVersion, ConvertedDocument, FormatKind, VisualProvenance};

    /// Builds a minimal but valid XLSX package with inline strings, so the
    /// fixture stays generated text rather than a checked-in binary.
    pub(crate) fn workbook(sheets: &[(&str, &[&[&str]])]) -> Vec<u8> {
        let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
        let mut sheet_entries = String::new();
        let mut content_overrides = String::new();
        for (index, (name, rows)) in sheets.iter().enumerate() {
            let number = index + 1;
            sheet_entries.push_str(&format!(
                r#"<sheet name="{name}" sheetId="{number}" r:id="rId{number}"/>"#
            ));
            content_overrides.push_str(&format!(
                r#"<Override PartName="/xl/worksheets/sheet{number}.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#
            ));
            let mut sheet_data = String::new();
            for (row_index, row) in rows.iter().enumerate() {
                let row_number = row_index + 1;
                sheet_data.push_str(&format!("<row r=\"{row_number}\">"));
                for (column_index, value) in row.iter().enumerate() {
                    let column = char::from(b'A' + column_index as u8);
                    sheet_data.push_str(&format!(
                        "<c r=\"{column}{row_number}\" t=\"inlineStr\"><is><t>{value}</t></is></c>"
                    ));
                }
                sheet_data.push_str("</row>");
            }
            entries.push((
                format!("xl/worksheets/sheet{number}.xml"),
                format!(
                    r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{sheet_data}</sheetData></worksheet>"#
                )
                .into_bytes(),
            ));
        }
        let relationships = sheets
            .iter()
            .enumerate()
            .map(|(index, _)| {
                let number = index + 1;
                format!(
                    r#"<Relationship Id="rId{number}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet{number}.xml"/>"#
                )
            })
            .collect::<String>();
        entries.push((
            "xl/workbook.xml".to_owned(),
            format!(
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets>{sheet_entries}</sheets></workbook>"#
            )
            .into_bytes(),
        ));
        entries.push((
            "xl/_rels/workbook.xml.rels".to_owned(),
            format!(
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{relationships}</Relationships>"#
            )
            .into_bytes(),
        ));
        entries.push((
            "_rels/.rels".to_owned(),
            br#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#.to_vec(),
        ));
        entries.push((
            "[Content_Types].xml".to_owned(),
            format!(
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>{content_overrides}</Types>"#
            )
            .into_bytes(),
        ));
        let borrowed: Vec<(&str, &[u8])> = entries
            .iter()
            .map(|(name, data)| (name.as_str(), data.as_slice()))
            .collect();
        package(&borrowed)
    }

    fn convert_workbook(bytes: &[u8]) -> Result<ConvertedDocument, WorkbookError> {
        let budgets = ConversionBudgets::default();
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Spreadsheet,
            &mut tracker,
        );
        convert(&mut builder, bytes)?;
        Ok(builder.finish())
    }

    #[test]
    fn sheets_become_bands_with_exact_cell_ranges() {
        let bytes = workbook(&[(
            "Sales",
            &[
                &["Region", "Total"] as &[&str],
                &["North", "12"],
                &["South", "7"],
            ],
        )]);
        let document = convert_workbook(&bytes).expect("conversion");
        assert_eq!(document.units().len(), 1);
        let unit = &document.units()[0];
        assert_eq!(unit.text, "Region | Total\nNorth | 12\nSouth | 7");
        assert_eq!(unit.section_path, ["Sales"]);
        assert_eq!(unit.boundary, TopLevelBoundary::Sheet("Sales".to_owned()));
        assert_eq!(
            unit.provenance,
            Provenance::SpreadsheetRange {
                sheet: "Sales".to_owned(),
                start_row: 0,
                start_column: 0,
                end_row: 2,
                end_column: 1
            }
        );
    }

    #[test]
    fn every_sheet_is_visited_in_workbook_order() {
        let rows: &[&[&str]] = &[&["a"]];
        let bytes = workbook(&[("First", rows), ("Second", rows)]);
        let document = convert_workbook(&bytes).expect("conversion");
        let sheets: Vec<&str> = document
            .units()
            .iter()
            .map(|unit| match &unit.provenance {
                Provenance::SpreadsheetRange { sheet, .. } => sheet.as_str(),
                other => panic!("unexpected provenance {other:?}"),
            })
            .collect();
        assert_eq!(sheets, ["First", "Second"]);
    }

    #[test]
    fn long_sheets_are_split_into_bands() {
        let rows: Vec<Vec<String>> = (0..40)
            .map(|index| vec![format!("row{index}"), index.to_string()])
            .collect();
        let borrowed_rows: Vec<Vec<&str>> = rows
            .iter()
            .map(|row| row.iter().map(String::as_str).collect())
            .collect();
        let row_slices: Vec<&[&str]> = borrowed_rows.iter().map(Vec::as_slice).collect();
        let bytes = workbook(&[("Data", row_slices.as_slice())]);
        let document = convert_workbook(&bytes).expect("conversion");
        assert_eq!(document.units().len(), 2);
        assert_eq!(
            document.units()[1].provenance,
            Provenance::SpreadsheetRange {
                sheet: "Data".to_owned(),
                start_row: 32,
                start_column: 0,
                end_row: 39,
                end_column: 1
            }
        );
    }

    #[test]
    fn a_broken_workbook_is_malformed() {
        let bytes = package(&[("xl/workbook.xml", b"<workbook/>")]);
        assert!(matches!(
            convert_workbook(&bytes),
            Err(WorkbookError::Malformed(_))
        ));
    }

    #[test]
    fn worksheet_drawing_images_follow_the_declared_relationship_chain() {
        let image = png();
        let bytes = package(&[
            (
                "xl/workbook.xml",
                br#"<workbook xmlns:r="r"><sheets><sheet r:id="rSheet"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                br#"<Relationships><Relationship Id="rSheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                br#"<worksheet xmlns:r="r"><drawing r:id="rDrawing"/></worksheet>"#,
            ),
            (
                "xl/worksheets/_rels/sheet1.xml.rels",
                br#"<Relationships><Relationship Id="rDrawing" Target="../drawings/drawing1.xml"/></Relationships>"#,
            ),
            (
                "xl/drawings/drawing1.xml",
                br#"<drawing xmlns:r="r"><blip r:embed="rImage"/></drawing>"#,
            ),
            (
                "xl/drawings/_rels/drawing1.xml.rels",
                br#"<Relationships><Relationship Id="rImage" Target="../media/picture.png"/></Relationships>"#,
            ),
            ("xl/media/picture.png", &image),
        ]);
        let budgets = ConversionBudgets::default();
        let cancellation = Cancellation::none();
        let clock = ManualClock::new();
        let mut tracker = BudgetTracker::new(&budgets, &cancellation, &clock);
        let mut archive = package::preflight(&bytes, &mut tracker).expect("preflight");
        let mut builder = DocumentBuilder::new(
            ComponentVersion::new("baseline", 1),
            FormatKind::Spreadsheet,
            &mut tracker,
        );
        builder.set_visuals_enabled(true);

        extract_visuals(&mut builder, &mut archive).expect("visual extraction");
        let converted = builder.finish();

        assert_eq!(converted.visuals().len(), 1);
        assert_eq!(
            converted.visuals()[0].provenance,
            VisualProvenance::SpreadsheetImage {
                sheet_index: 0,
                image_index: 0
            }
        );
    }
}
